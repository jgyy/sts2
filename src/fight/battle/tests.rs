use super::testing::*;
use super::*;

const HITS: [(i64, i64, bool, bool); 6] = [(6, 1, false, false), (4, 3, false, false), (9, 2, true, false), (30, 1, false, false), (7, 1, false, true), (100, 2, false, false)];

#[test]
fn golden_states() {
    let mut checked = 0;
    for (n, row) in states().iter().enumerate() {
        let _env = env(row);
        let (state, r) = (&row["s"], &row["r"]);
        let (player, listed) = (&state["player"], state["battle"]["enemies"].list());
        let round_no = if state["battle"]["round"].truthy() { state["battle"]["round"].int() } else { 1 };
        let why = |what: &str| format!("state {n}: {what}");
        let es = read_enemies(state);
        assert_eq!(es.len(), r["read_enemies"].list().len(), "{}", why("read_enemies"));
        for (e, want) in es.iter().zip(r["read_enemies"].list()) {
            assert_eq!(*e, Enemy::from_json(&want[0]), "{}", why("read_enemies"));
            assert_eq!(json!([e.worth(), e.alive()]), json!([want[1], want[2]]), "{}", why("worth"));
        }
        let room = next_room(state);
        assert_eq!(json!([expected_loss(state), _sandpit(state), cards_left_this_turn(state), [room.0, room.1], free_plays(state)]), r["state"], "{}", why("state"));
        let (deck, draw) = (deck_count(player), _draw_worth(player));
        let burns: Vec<Value> = hand_burns(player).iter().map(|b| json!([b.0, b.1, b.2])).collect();
        let got = json!([end_of_turn_block(player), end_of_turn_loss(player), burns, osty_hp(player), constrict(player), [deck.0, deck.1], _bare_block(player), [draw.0, draw.1, draw.2], loss_cap(player)]);
        assert_eq!(got, r["player"], "{}", why("player"));
        let odds = |d: i64| {
            let o = _escape_odds(player, d);
            json!([o.0, o.1])
        };
        assert_eq!(json!([odds(5), odds(3), odds(7)]), r["escape"], "{}", why("escape"));
        let got = json!([_next_wither(player, 1), _next_wither(player, round_no), _next_wither(player, 8), _wither_bill(player, round_no, 3), _wither_bill(player, 2, 9)]);
        assert_eq!(got, r["wither"], "{}", why("wither"));
        for (c, rows) in std::iter::once(player).chain(listed).zip(r["powers"].list()) {
            for want in rows.list() {
                let pid = want[0].s();
                let got = json!([pid, _shown(c, pid, 0), _shown(c, pid, 1).unwrap_or(-1), power_amount(c, pid), _has_power(c, pid), _stacks(c, pid)]);
                assert_eq!(got, *want, "{}", why("powers"));
                checked += 5;
            }
        }
        let got: Vec<Vec<Value>> = listed.iter().map(|e| e["intents"].list().iter().map(|i| json!([_is_attack(i), intent_damage(i), intent_hits(i)])).collect()).collect();
        assert_eq!(json!(got), r["intents"], "{}", why("intents"));
        let got: Vec<Value> = listed.iter().map(|e| json!([_reroll(e, player, 20), _reroll(e, player, 32)])).collect();
        assert_eq!(json!(got), r["reroll"], "{}", why("reroll"));
        let hand: Vec<CardInfo> = player["hand"].list().iter().map(parse).collect();
        for (c, want) in hand.iter().zip(r["hand"].list()) {
            assert_eq!(json!([_power_worth(c, true), _power_worth(c, false)]), json!([want[5], want[6]]), "{}", why(&c.key));
        }
        let playable: Vec<CardInfo> = hand.iter().filter(|c| c.playable).cloned().collect();
        let got: Vec<Value> = [0, player["energy"].int(), 3].iter().map(|n| _set_up(&playable, *n).map(|a| json!([a.0, a.1])).unwrap_or(Value::Null)).collect();
        assert_eq!(json!(got), r["set_up"], "{}", why("set_up"));
        let got: Vec<Option<usize>> = TARGETS.iter().map(|(d, h, dv)| _pick_target(&es, *d, *h, *dv)).collect();
        assert_eq!(json!(got), r["target"], "{}", why("target"));
        let refs: Vec<&Enemy> = es.iter().collect();
        let all: Vec<usize> = (0..es.len()).collect();
        for ((d, h, dv), want) in TARGETS.iter().zip(&got) {
            assert_eq!(_pick_target(&refs, *d, *h, *dv), *want, "{}", why("target by reference"));
            assert_eq!(_pick_target_among(&es, &all, *d, *h, *dv), *want, "{}", why("target among all"));
        }
        let got: Vec<Vec<bool>> = TARGETS.iter().map(|(d, h, dv)| es.iter().map(|e| _would_kill(e, *d, *h, *dv)).collect()).collect();
        assert_eq!(json!(got), r["kill"], "{}", why("kill"));
        assert_eq!(json!(es.iter().map(_fated).collect::<Vec<_>>()), r["fated"], "{}", why("fated"));
        assert_eq!(json!([fj(kill_reach(&es, true)), fj(kill_reach(&es, false))]), r["reach"], "{}", why("reach"));
        for ((d, h, dv, raw), rows) in HITS.iter().zip(r["hits"].list()) {
            for (e, want) in es.iter().zip(rows.list()) {
                let mut e2 = clone(e);
                let per = _per_hit(&e2, *d, *raw);
                let removed = _hit(&mut e2, *d, *h, *dv, *raw);
                assert_eq!(e2, Enemy::from_json(&want[2]), "{}", why("hit"));
                let first = _progress(&mut e2, removed, 0.25);
                let second = _progress(&mut Enemy { chipped: 3, stripped: 1, ..clone(&e2) }, removed, 1.0);
                assert_eq!(json!([per, removed, want[2], first, second]), *want, "{}", why("hit"));
                checked += 5;
            }
        }
        let mut gone: Vec<Enemy> = es.iter().map(|e| Enemy { hp: 0, ..clone(e) }).collect();
        _respawns(&mut gone);
        assert_eq!(gone, r["respawn"].list().iter().map(Enemy::from_json).collect::<Vec<_>>(), "{}", why("respawn"));
        let mut crabs: Vec<Enemy> = es.iter().map(clone).collect();
        let mut dead: HashSet<Arc<str>> = HashSet::new();
        if !crabs.is_empty() {
            crabs[0].hp = 0;
        }
        _rage_blocks(&mut crabs, &mut dead);
        let mut dead: Vec<&str> = dead.iter().map(|d| &**d).collect();
        dead.sort();
        assert_eq!(json!([crabs.iter().map(|e| e.block).collect::<Vec<_>>(), dead]), r["rage"], "{}", why("rage"));
        checked += 30 + es.len() * 12 + hand.len() * 2;
    }
    assert!(checked > 20_000, "{checked} checks");
}

#[test]
fn golden_calls() {
    let g: Value = serde_json::from_str(include_str!("../../../tests/golden/fight/battle.json")).unwrap();
    let mut checked = 0;
    let mut each = |name: &str, at_least: usize, check: &dyn Fn(&Value, &Value) -> (Value, Value)| {
        for (n, rec) in calls(&g, name, at_least).iter().enumerate() {
            let _env = env(&rec["env"]);
            let (got, want) = check(&rec["a"], rec);
            assert_eq!(got, want, "{name} case {n}: {}", base::json_dumps(&rec["a"]));
            checked += 1;
        }
    };
    each("battle.reacted", 30, &|a, rec| (json!(reacted(a[0].int(), a[1].int(), a[2].int())), rec["r"].clone()));
    each("battle._hit_count", 20, &|a, rec| (json!(_hit_count(&foe(&a[0]), a[1].int(), a[2].truthy())), rec["r"].clone()));
    each("battle._per_hit", 300, &|a, rec| (json!(_per_hit(&foe(&a[0]), a[1].int(), a[2].truthy())), rec["r"].clone()));
    each("battle._would_kill", 300, &|a, rec| (json!(_would_kill(&foe(&a[0]), a[1].int(), a[2].int(), a[3].truthy())), rec["r"].clone()));
    each("battle._fated", 300, &|a, rec| (json!(_fated(&foe(&a[0]))), rec["r"].clone()));
    each("battle._hit", 500, &|a, rec| {
        let mut e = foe(&a[0]);
        let removed = _hit(&mut e, a[1].int(), a[2].int(), a[3].truthy(), a[4].truthy());
        (json!([removed, e.to_json()]), json!([rec["r"], foe(&rec["m"][0]).to_json()]))
    });
    each("battle._progress", 400, &|a, rec| {
        let mut e = foe(&a[0]);
        let gained = _progress(&mut e, a[1].int(), a[2].float());
        (json!([gained, e.to_json()]), json!([rec["r"], foe(&rec["m"][0]).to_json()]))
    });
    each("battle._resists", 300, &|a, rec| {
        let mut e = foe(&a[0]);
        let held = _resists(&mut e);
        (json!([held, e.to_json()]), json!([rec["r"], foe(&rec["m"][0]).to_json()]))
    });
    each("battle._pick_target", 400, &|a, rec| (json!(_pick_target(&foes(&a[0]), a[1].int(), a[2].int(), a[3].truthy())), rec["r"].clone()));
    each("battle.kill_reach", 250, &|a, rec| (fj(kill_reach(&foes(&a[0]), a[1].truthy())), rec["r"].clone()));
    each("battle._respawns", 200, &|a, rec| {
        let mut all = foes(&a[0]);
        _respawns(&mut all);
        (json!(all.iter().map(Enemy::to_json).collect::<Vec<_>>()), json!(foes(&rec["m"][0]).iter().map(Enemy::to_json).collect::<Vec<_>>()))
    });
    each("battle._rage_blocks", 200, &|a, rec| {
        let mut all = foes(&a[0]);
        let mut dead: HashSet<Arc<str>> = a[1].list().iter().map(|d| Arc::from(d.s())).collect();
        _rage_blocks(&mut all, &mut dead);
        let mut dead: Vec<&str> = dead.iter().map(|d| &**d).collect();
        dead.sort();
        (json!([all.iter().map(Enemy::to_json).collect::<Vec<_>>(), dead]), json!([foes(&rec["m"][0]).iter().map(Enemy::to_json).collect::<Vec<_>>(), rec["m"][1]]))
    });
    each("battle._set_focus", 200, &|a, rec| {
        let mut all = foes(&a[0]);
        _set_focus(&mut all);
        (json!(all.iter().map(Enemy::to_json).collect::<Vec<_>>()), json!(foes(&rec["m"][0]).iter().map(Enemy::to_json).collect::<Vec<_>>()))
    });
    each("battle.junk_cost", 100, &|a, rec| (json!(junk_cost(a[0].s(), a[1].int(), (a[2][0].int(), a[2][1].int()), a[3].int())), rec["r"].clone()));
    each("battle.next_energy_worth", 70, &|a, rec| (json!(next_energy_worth(a[0].int(), (a[1][0].float(), a[1][1].float()), a[2].int())), rec["r"].clone()));
    each("battle._poison_total", 30, &|a, rec| (json!(_poison_total(a[0].int(), a[1].int())), rec["r"].clone()));
    each("battle._power_worth", 30, &|a, rec| (json!(_power_worth(&card_from_json(&a[0]), a[1].truthy())), rec["r"].clone()));
    each("battle._set_up", 200, &|a, rec| (_set_up(&cards_from_json(&a[0]), a[1].int()).map(|x| json!([x.0, x.1])).unwrap_or(Value::Null), rec["r"].clone()));
    assert!(checked > 4000, "{checked} cases");
}

#[test]
fn golden_cards() {
    let g: Value = serde_json::from_str(include_str!("../../../tests/golden/fight/cards.json")).unwrap();
    let rows = calls(&g, "cards", 1200);
    for (game, field) in [("sts2", "pw"), ("sts1", "pw1")] {
        let _game = crate::games::playing(game);
        for row in rows {
            let c = card_from_json(&row["c"]);
            assert_eq!(json!([_power_worth(&c, true), _power_worth(&c, false)]), row[field], "{game} {}", c.name);
        }
    }
}

#[test]
fn the_tables_can_be_put_aside() {
    let state = json!({"state_type": "monster", "player": {"hp": 50}, "battle": {"round": 1, "enemies": [
        {"entity_id": "KIN_PRIEST_0", "hp": 190, "max_hp": 190, "intents": [{"type": "Attack", "label": "5"}]},
        {"entity_id": "KIN_FOLLOWER_0", "hp": 58, "max_hp": 58, "status": [{"id": "MINION_POWER", "amount": null}], "intents": [{"type": "Attack", "label": "5"}]},
        {"entity_id": "ROCKET_1", "hp": 190, "max_hp": 190, "intents": [{"type": "Buff"}]}]}});
    let read = || {
        let es = read_enemies(&state);
        (es[1].main_threat, es[2].threat, es[0].focus)
    };
    // (the first version: battle.read_enemies(state) -> main_threat True, threat 20.0, focus 6/26)
    assert_eq!(read(), (true, 20.0, 6.0 / 26.0));
    {
        let _minions = with_main_minions(&[]);
        let _threat = with_known_threat(IndexMap::from([("KIN_PRIEST".to_string(), 30.0)]));
        assert_eq!(read(), (false, 0.0, 1.0));
        assert_eq!((known_threat_of("ROCKET"), known_threat_of("KIN_PRIEST"), is_main_minion("KIN_FOLLOWER")), (None, Some(30.0), false));
    }
    assert_eq!(read(), (true, 20.0, 6.0 / 26.0));
    assert_eq!((known_threat().len(), main_minions().len()), (6, 1));
    let mut crabs = vec![Enemy { entity_id: "A".into(), hp: 0, ..Enemy::default() }, Enemy { entity_id: "B".into(), hp: 9, rage: true, ..Enemy::default() }];
    {
        let _none = RAGE_BLOCK.set(0);
        _rage_blocks(&mut crabs.clone(), &mut HashSet::new());
        let mut calm = crabs.clone();
        _rage_blocks(&mut calm, &mut HashSet::new());
        assert_eq!(calm[1].block, 0);
    }
    _rage_blocks(&mut crabs, &mut HashSet::new());
    assert_eq!(crabs[1].block, 99);
}
