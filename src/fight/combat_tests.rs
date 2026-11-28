//! The planner's own tests (combat.rs's `tests` module, in a file of its own).

use super::testing::*;
use super::*;

#[test]
fn golden_choose_card() {
    let g = golden();
    let cases = rows(
        &[
            include_str!("../../tests/golden/combat/choose0.json"),
            include_str!("../../tests/golden/combat/choose1.json"),
            include_str!("../../tests/golden/combat/choose2.json"),
        ],
        "choose",
    );
    assert!(cases.len() >= 3000, "{} cases", cases.len());
    let (mut played, mut bad) = (0, Vec::new());
    for (n, rec) in cases.iter().enumerate() {
        let _env = g.env(&rec["env"]);
        let state = g.state(&rec["s"]);
        let mut plan = Plan::default();
        let got = _choose_card(&state, Some(&mut plan));
        played += got.is_some() as usize;
        let want = &rec["plan"];
        let line: Option<Vec<i64>> = want["line"].as_array().map(|l| l.iter().map(|i| i.int()).collect());
        let same = action(&got) == rec["r"]
            && plan.lost == want["lost"].as_i64()
            && plan.score == want["score"].as_f64()
            && plan.line.as_ref().map(|l| l.iter().map(|c| c.index).collect::<Vec<_>>()) == line
            && (!rec.has("ctx") || SimulateArgs { vulnerable: false, .._context(&state) } == args(&rec["ctx"])) // (newer than the recordings)
            && (!rec.has("describe") || json!(describe(&state)) == rec["describe"])
            && json!(expected_end_loss(&state)) == rec["end"]
            && json!(fortifier(&state["player"])) == rec["fort"]
            && _wrath_outlook(&state) == wrath(&rec["wrath"]);
        if !same {
            if bad.len() < 5 {
                eprintln!(
                    "case {n} ({}): got {} lost {:?} score {:?} line {:?}\n  want {} plan {}\n  ctx {}\n  describe {}",
                    rec["env"],
                    action(&got),
                    plan.lost,
                    plan.score,
                    plan.line.as_ref().map(|l| l.iter().map(|c| c.index).collect::<Vec<_>>()),
                    rec["r"],
                    want,
                    !rec.has("ctx") || _context(&state) == args(&rec["ctx"]),
                    describe(&state),
                );
            }
            bad.push(n);
        }
    }
    assert!(bad.is_empty(), "{} of {} cases differ: {:?}", bad.len(), cases.len(), &bad[..bad.len().min(40)]);
    assert!(played > 1500, "{played} cards played");
}

#[test]
fn golden_simulate() {
    let g = golden();
    let cases = rows(&[include_str!("../../tests/golden/combat/sim0.json"), include_str!("../../tests/golden/combat/sim1.json")], "sim");
    assert!(cases.len() >= 3000, "{} cases", cases.len());
    let (mut none, mut bad) = (0, Vec::new());
    for (n, rec) in cases.iter().enumerate() {
        let _env = g.env(&rec["env"]);
        let line = g.line(&rec["line"]).unwrap();
        let foes: Vec<Enemy> = rec["foes"].list().iter().map(Enemy::from_json).collect();
        let before = foes.clone();
        let got = simulate(&line, &foes, args(&rec["kw"]));
        assert_eq!(foes, before);
        none += got.is_none() as usize;
        let shown = match &got {
            Some(out) => json!([out.score, out.first_target.as_deref(), out.lost]),
            None => Value::Null,
        };
        if shown != rec["r"] {
            if bad.len() < 5 {
                eprintln!("case {n}: got {shown} want {}\n  {}", rec["r"], rec);
            }
            bad.push(n);
        }
    }
    assert!(bad.is_empty(), "{} of {} cases differ: {:?}", bad.len(), cases.len(), &bad[..bad.len().min(40)]);
    assert!(none > 100, "{none} lines that cannot be played");
}

#[test]
fn golden_actions() {
    let g = golden();
    let cases = rows(&[CALLS], "actions");
    assert!(cases.len() >= 300, "{} cases", cases.len());
    let mut potions = 0;
    for (n, rec) in cases.iter().enumerate() {
        let _env = g.env(&rec["env"]);
        let got = actions(&g.state(&rec["s"]));
        potions += got.iter().filter(|a| a.0 == "use_potion").count();
        assert_eq!(actions_json(&got), rec["r"], "case {n}");
    }
    assert!(potions > 50, "{potions} potions drunk");
}

#[test]
fn golden_small() {
    let g = golden();
    let cases = rows(&[CALLS], "small");
    let mut seen: HashMap<String, usize> = HashMap::new();
    for (n, rec) in cases.iter().enumerate() {
        let _env = g.env(&rec["env"]);
        let (a, m) = (&rec["a"], &rec["m"]);
        *seen.entry(rec["f"].s().to_string()).or_default() += 1;
        match rec["f"].s() {
            "_orb_damage" => {
                let mut foes: Vec<Enemy> = a[2].list().iter().map(Enemy::from_json).collect();
                let got = _orb_damage(a[0].s(), a[1].int(), &mut foes, a[3].float(), a[4].float());
                assert_eq!(json!(got), rec["r"], "case {n}: {rec}");
                assert_eq!(foes, m[2].list().iter().map(Enemy::from_json).collect::<Vec<_>>(), "case {n}");
            }
            "_debuff" => {
                let mut e = Enemy::from_json(&a[0]);
                _debuff(&mut e, &g.cards[a[1].int() as usize], a[2].int());
                assert_eq!(e, Enemy::from_json(&m[0]), "case {n}: {rec}");
            }
            "_expose" => {
                let mut e = Enemy::from_json(&a[0]);
                _expose(&mut e, a[1].int());
                assert_eq!(e, Enemy::from_json(&m[0]), "case {n}: {rec}");
            }
            "_weaken" => {
                let mut e = Enemy::from_json(&a[0]);
                _weaken(&mut e, a[1].int());
                assert_eq!(e, Enemy::from_json(&m[0]), "case {n}: {rec}");
            }
            other => panic!("no function {other}"),
        }
    }
    for f in ["_orb_damage", "_debuff", "_expose", "_weaken"] {
        assert!(seen.get(f).copied().unwrap_or(0) >= 100, "{f}: {:?}", seen.get(f));
    }
}

#[test]
fn the_orders_of_itertools() {
    let mut picked = vec![0, 1];
    let mut all = vec![picked.clone()];
    while next_combination(&mut picked, 4) {
        all.push(picked.clone());
    }
    assert_eq!(all, [[0, 1], [0, 2], [0, 3], [1, 2], [1, 3], [2, 3]]);
    let mut order = vec![0, 1, 2];
    let mut all = vec![order.clone()];
    while next_permutation(&mut order) {
        all.push(order.clone());
    }
    assert_eq!(all, [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]]);
    let mut one = vec![0];
    assert!(!next_permutation(&mut one) && !next_combination(&mut one, 1));
}

#[test]
fn the_hooks_and_the_constants() {
    let state = json!({"player": {"hp": 50, "max_hp": 80, "energy": 3, "hand": []}, "battle": {"round": 1, "enemies": []}});
    assert_eq!(next_room(&state), (99.0, 1.0));
    {
        let _room = with_next_room(|_| (0.0, 1.0));
        assert_eq!(_context(&state).room, (0.0, 1.0));
    }
    assert_eq!(_context(&state).room, (99.0, 1.0));
    let calls = Rc::new(std::cell::Cell::new(0));
    {
        let spy = calls.clone();
        let _hook = with_choose_card_hook(move |s, plan| {
            spy.set(spy.get() + 1);
            _choose_card(s, plan)
        });
        assert_eq!(actions(&state), vec![("end_turn".to_string(), json!({}))]);
    }
    assert_eq!((calls.get(), choose_card(&state, None)), (1, None));
    assert_eq!((MAX_SEARCH_CARDS.get(), HP_BASE.get(), HP_SLOPE.get(), HP_BITE.get(), HIVE_COST.get(), HIVE_PACE.get()), (11, 1.5, 2.5, 3.0, 9.0, 22.0));
    assert_eq!((DEBUFF_CAP.get(), FORM_WORTH.get(), ENRAGE_PRICE.get()), (4, 40.0, 2.0));
    assert!(TUNABLES.contains(&"HP_BASE") && !set_tunable("NO_SUCH", 1.0));
    let defaults = SimulateArgs::default();
    assert_eq!((defaults.fight.as_str(), defaults.round_no, defaults.pet, defaults.arrows, defaults.fortify, defaults.shiv_damage, defaults.wither_hp), ("monster", 1, -1, -1, 1, 4, 5));
    assert_eq!((defaults.draw_worth, defaults.escape_odds, defaults.room, defaults.deck, defaults.wither_bill), ((2.0, 0.0, 0.0), (0.3, 0.3), (99.0, 1.0), (20, 0), 7.0));
}
