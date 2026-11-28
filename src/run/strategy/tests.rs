use super::*;
use crate::games;
use crate::learn;

/// A case as it was recorded: `{"$r": n}` stands for `blobs[n]` (with `"$i"`, a dict whose first key is that index).
fn resolve(v: &Value, blobs: &[Value]) -> Value {
    match v {
        Value::Object(m) if m.contains_key("$r") => {
            let inner = resolve(&blobs[m["$r"].as_u64().unwrap() as usize], blobs);
            match m.get("$i") {
                Some(index) => {
                    let mut out = serde_json::Map::new();
                    out.insert("index".into(), index.clone());
                    out.extend(inner.as_object().unwrap().clone());
                    Value::Object(out)
                }
                None => inner,
            }
        }
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), resolve(x, blobs))).collect()),
        Value::Array(a) => Value::Array(a.iter().map(|x| resolve(x, blobs)).collect()),
        other => other.clone(),
    }
}

/// `callable(card) -> gain` from the answers the original gave for the cards it was asked about.
fn table(rows: &Value) -> UpgradeGain {
    let rows: Vec<(Value, Option<f64>)> = rows.list().iter().map(|r| (r[0].clone(), r[1].as_f64())).collect();
    Rc::new(move |card: &Value| rows.iter().find(|(c, _)| c == card).unwrap_or_else(|| panic!("no recorded gain for {card}")).1)
}

fn where_of(k: &Value) -> Where {
    (k[0].as_i64(), k[1].as_i64())
}

fn nodes_of(v: &Value) -> IndexMap<(i64, i64), Value> {
    match v.get("$nodes") {
        Some(nodes) => nodes.list().iter().map(|n| ((n["col"].as_i64().unwrap(), n["row"].as_i64().unwrap()), n.clone())).collect(),
        None => v.list().iter().map(|kn| ((kn[0][0].as_i64().unwrap(), kn[0][1].as_i64().unwrap()), kn[1].clone())).collect(),
    }
}

/// The test-only JSON form of a Memory (the original capture's `mem_json`), read back.
fn mem_of(j: &Value, gain: UpgradeGain) -> Memory {
    let strings = |v: &Value| -> Vec<String> { v.list().iter().map(base::str_of).collect() };
    Memory {
        deck_size: j["deck_size"].as_i64().unwrap(),
        deck: j["deck"].list().to_vec(),
        max_energy: j["max_energy"].as_i64().unwrap(),
        picks: j["picks"].list().iter().map(|p| (p[0].int(), base::str_of(&p[1]))).collect(),
        card_stats: None,
        card_skips: j["card_skips"].list().iter().map(|kn| (where_of(&kn[0]), kn[1].as_i64().unwrap())).collect(),
        select_key: j["select_key"].s().into(),
        logged_select: j["logged_select"].s().into(),
        logged_shop: j["logged_shop"].s().into(),
        selected: j["selected"].list().iter().map(|i| i.as_i64().unwrap()).collect(),
        runs_finished: j["runs_finished"].as_i64().unwrap(),
        character_selected: j["character_selected"].truthy(),
        boss_next: j["boss_next"].truthy(),
        event_id: j["event_id"].s().into(),
        event_hp0: j["event_hp0"].as_i64().unwrap(),
        map_nodes: nodes_of(&j["map_nodes"]),
        map_at: pair(&j["map_at"]),
        upgrade_gain: if j["upgrade_gain"].is_null() { None } else { Some(gain) },
        over_waits: j["over_waits"].as_i64().unwrap(),
        max_ascension: j["max_ascension"].entries().map(|(k, v)| (k.clone(), v.int())).collect(),
        ascension_waits: j["ascension_waits"].as_i64().unwrap(),
        epochs: j["epochs"].entries().map(|(k, v)| (k.clone(), base::str_of(v))).collect(),
        last_result: j["last_result"].s().into(),
        recent: strings(&j["recent"]),
        modes: Default::default(),
        timeline_done: j["timeline_done"].truthy(),
        timeline_stuck: strings(&j["timeline_stuck"]).into_iter().collect(),
        parasol: match j["parasol"].list() {
            [act, floor, looks] => Some((act.as_i64(), floor.as_i64(), looks.as_i64().unwrap())),
            _ => None,
        },
        potion_swaps: j["potion_swaps"].list().iter().map(|kn| (where_of(&kn[0]), kn[1].as_i64().unwrap())).collect(),
    }
}

fn dict<'a>(items: impl Iterator<Item = (&'a String, Value)>) -> Value {
    Value::Object(items.map(|(k, v)| (k.clone(), v)).collect())
}

/// ... and written (the two tables kept by place in a fixed order: Rust keeps them unordered).
fn mem_json(m: &Memory) -> Value {
    let by_place = |t: &HashMap<Where, i64>| -> Value {
        let mut rows: Vec<(Where, i64)> = t.iter().map(|(k, n)| (*k, *n)).collect();
        rows.sort();
        json!(rows.iter().map(|((act, floor), n)| json!([[act, floor], n])).collect::<Vec<_>>())
    };
    json!({
        "deck_size": m.deck_size, "deck": m.deck, "max_energy": m.max_energy, "picks": m.picks.iter().map(|(floor, name)| json!([floor, name])).collect::<Vec<_>>(),
        "card_skips": by_place(&m.card_skips), "select_key": m.select_key, "logged_select": m.logged_select, "logged_shop": m.logged_shop, "selected": m.selected,
        "runs_finished": m.runs_finished, "character_selected": m.character_selected, "boss_next": m.boss_next, "event_id": m.event_id, "event_hp0": m.event_hp0,
        "map_nodes": m.map_nodes.iter().map(|((col, row), n)| json!([[col, row], n])).collect::<Vec<_>>(), "map_at": m.map_at.map_or(json!([]), |(col, row)| json!([col, row])),
        "upgrade_gain": m.upgrade_gain.is_some(), "over_waits": m.over_waits, "max_ascension": dict(m.max_ascension.iter().map(|(k, v)| (k, json!(v)))),
        "ascension_waits": m.ascension_waits, "epochs": dict(m.epochs.iter().map(|(k, v)| (k, json!(v)))),
        "last_result": m.last_result, "recent": m.recent, "timeline_done": m.timeline_done, "timeline_stuck": m.timeline_stuck,
        "parasol": m.parasol.map_or(json!([]), |(act, floor, looks)| json!([act, floor, looks])), "potion_swaps": by_place(&m.potion_swaps),
    })
}

fn actions(out: Vec<Action>) -> Value {
    json!(out.into_iter().map(|(name, params)| json!([name, params])).collect::<Vec<_>>())
}

fn strings(v: &Value) -> Vec<String> {
    v.list().iter().map(base::str_of).collect()
}

fn epochs_of(v: &Value) -> Option<IndexMap<String, String>> {
    v.dict().map(|d| d.iter().map(|(k, x)| (k.clone(), base::str_of(x))).collect())
}

fn rooms_of(v: &Value) -> Vec<(String, Option<i64>)> {
    v.list().iter().map(|r| (r[0].s().to_string(), r[1].as_i64())).collect()
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

fn i(v: &Value) -> i64 {
    v.as_i64().unwrap()
}

/// Every handler and helper as the original tests and the model runs of both games called them (see the capture notes in the
/// commit that added tests/golden/strategy/): the same actions, the same numbers, the same Memory afterwards.
fn replay(text: &str) -> usize {
    let g: Value = serde_json::from_str(text).unwrap();
    let blobs = g["blobs"].list();
    let mut seen = 0;
    for case in g["cases"].list() {
        let case = resolve(case, blobs);
        let name = case["fn"].s();
        let _game = games::playing(case["game"].s());
        let _newer = (EMERALD_HOLD.set(0.0), crate::run::sphere::SPHERE_AIM.set(false)); // (the recordings are older than these rules)
        let knob: IndexMap<String, f64> = case["knob"].entries().map(|(k, v)| (k.clone(), v.float())).collect();
        let before = learn::knobs();
        learn::set_knobs(&knob);
        set_session(case["session"].as_i64());
        afflictions::_OFFER_reset();
        handpick::_answers_clear();
        let a = &case["args"];
        let state = &a["state"];
        let mut mem = if case.has("mem") { mem_of(&case["mem"], table(&case["gains"]["mem"])) } else { Memory::new() };
        let gain = if a["gain"].is_null() { None } else { Some(table(&case["gains"]["gain"])) };
        let got: Value = match name {
            "_where" => {
                let (act, floor) = _where(state);
                json!([act, floor])
            }
            "_vitals" => {
                let (player, hp, max_hp) = _vitals(state);
                json!([if player.is_null() { json!({}) } else { player.clone() }, hp, max_hp])
            }
            "_option_names" => json!(_option_names(&a["options"])),
            "menu" => actions(menu(state, &mut mem, a["character"].s())),
            "timeline_complete" => json!(timeline_complete(epochs_of(&a["epochs"]).as_ref())),
            "missing_epochs" => json!(missing_epochs(a["character"].s(), epochs_of(&a["epochs"]).as_ref())),
            "_who" => json!(_who(&base::str_of(&a["name"]))),
            "choose_character" => {
                let levels: Option<IndexMap<String, i64>> = a["max_ascension"].dict().map(|d| d.iter().map(|(k, v)| (k.clone(), v.int())).collect());
                let recent = a["recent"].as_array().map(|_| strings(&a["recent"]));
                json!(choose_character(a["setting"].s(), &strings(&a["unlocked"]), i(&a["runs_finished"]), levels.as_ref(), epochs_of(&a["epochs"]).as_ref(), recent.as_deref()))
            }
            "_room_value" => json!(_room_value(a["kind"].s(), f(&a["hp"]), i(&a["gold"]), a["early"].truthy(), a["act"].as_i64())),
            "_between" => {
                let points: Vec<(f64, f64)> = a["points"].list().iter().map(|p| (f(&p[0]), f(&p[1]))).collect();
                json!(_between(&points, f(&a["x"])))
            }
            "boss_odds" => json!(boss_odds(f(&a["hp_ratio"]), i(&a["act"]))),
            "_upgrade_worth" => json!(_upgrade_worth(a["act"].as_i64())),
            "path_value" => json!(path_value(&rooms_of(&a["rooms"]), f(&a["hp"]), i(&a["max_hp"]), i(&a["gold"]), i(&a["act"]), f(&a["harsh"]))),
            "best_onward" => {
                let mut budget = i(&a["budget"][0]);
                let got = best_onward(&nodes_of(&a["nodes"]), pair(&a["key"]), &rooms_of(&a["rooms"]), f(&a["hp"]), i(&a["max_hp"]), i(&a["gold"]), i(&a["act"]), &mut budget);
                assert_eq!(json!([budget]), case["budget_after"], "best_onward's budget {a}");
                json!(got)
            }
            "map_screen" => actions(map_screen(state, &mut mem)),
            "_belt_full" => json!(_belt_full(&a["player"])),
            "rewards" => actions(rewards(state, &mut mem)),
            "card_reward" => actions(card_reward(state, &mut mem)),
            "rest_site" => actions(rest_site(state, &mut mem)),
            "_shop_card" => _shop_card(&a["item"]),
            "shop" => actions(shop(state, &mut mem, a["key"].s())),
            "_removal_rank" => json!(_removal_rank(&a["card"])),
            "_pick_order" => json!(_pick_order(a["cards"].list(), a["prompt"].s(), a["screen_type"].s(), gain.as_ref())),
            "card_select" => actions(card_select(state, &mut mem)),
            "_put_back_order" => json!(_put_back_order(a["cards"].list(), a["energy"].as_i64())),
            "_keyword" => json!(_keyword(&a["c"], a["word"].s())),
            "_hand_kept" => json!(_hand_kept(state)),
            "_optional_pick" => json!(_optional_pick(state, &a["hs"], a["prompt"].s())),
            "hand_select" => actions(hand_select(state, &mut mem)),
            "bundle_select" => actions(bundle_select(state, &mut mem)),
            "relic_select" => actions(relic_select(state, &mut mem)),
            "treasure" => actions(treasure(state, &mut mem)),
            "crystal_sphere" => actions(crystal_sphere(state)),
            "game_over" => actions(game_over(state, &mut mem)),
            "overlay" => actions(overlay(state, &mut mem)),
            other => panic!("no replay for {other}"),
        };
        assert_eq!(got, case["out"], "{name} {} (game {}, knobs {})", a, case["game"], case["knob"]);
        if case.has("mem") {
            let mut want = case["mem"].clone();
            for (k, v) in case["mem_after"].entries() {
                want[k.as_str()] = v.clone();
            }
            assert_eq!(mem_json(&mem), mem_json(&mem_of(&want, table(&json!([])))), "{name}'s Memory afterwards {a}");
        }
        learn::set_knobs(&before);
        set_session(None);
        seen += 1;
    }
    seen
}

#[test]
fn golden_calls_0() {
    assert!(replay(include_str!("../../../tests/golden/strategy/cases0.json")) > 500);
}

#[test]
fn golden_calls_1() {
    assert!(replay(include_str!("../../../tests/golden/strategy/cases1.json")) > 200);
}

#[test]
fn golden_calls_2() {
    assert!(replay(include_str!("../../../tests/golden/strategy/cases2.json")) > 200);
}

#[test]
fn golden_calls_3() {
    assert!(replay(include_str!("../../../tests/golden/strategy/cases3.json")) > 100);
}
