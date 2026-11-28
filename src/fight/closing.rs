//! The end of a boss fight is a race: HP still held when the boss dies buys nothing (not so after an elite: the run goes on).

use std::borrow::Borrow;

use serde_json::Value;

use super::battle::Enemy;
use crate::base::JsonExt;

crate::tunables! {
    /// turns of our usual damage within which a boss counts as dying next turn (0: the rule is off)
    pub CLOSE_REACH: f64 = 1.0;
    /// ... and what our HP costs then, as a share of the usual price
    pub CLOSE_PRICE: f64 = 0.4;
}

/// HP the enemies have lost a round so far (0 before round 3, and outside boss fights).
pub fn pace(state: &Value) -> f64 {
    let battle = &state["battle"];
    let round_no = if battle["round"].truthy() { battle["round"].int() } else { 1 };
    if round_no < 3 || state["state_type"].text() != "boss" {
        return 0.0;
    }
    let gone: i64 = battle["enemies"].list().iter().filter(|e| e["max_hp"].int() < 100_000).map(|e| 0.max(e["max_hp"].int() - e["hp"].int())).sum();
    gone as f64 / (round_no - 1) as f64
}

/// What to multiply the price of the HP a line loses by: CLOSE_PRICE when the line leaves every leader within reach, else 1.
pub fn discount<E: Borrow<Enemy>>(pace: f64, lost: i64, hp: i64, survivors: &[E]) -> f64 {
    let leaders = || survivors.iter().map(|e| e.borrow()).filter(|e| !e.minion);
    if pace == 0.0 || CLOSE_REACH.get() == 0.0 || lost >= hp || leaders().next().is_none() || survivors.iter().map(|e| e.borrow()).any(|e| e.adapts || e.spent || e.respawns != 0) {
        return 1.0; // (a boss that comes back is no race; losing all our HP is never cheap)
    }
    if leaders().map(|e| e.hp + e.block).sum::<i64>() as f64 <= CLOSE_REACH.get() * pace {
        CLOSE_PRICE.get()
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fight::battle::read_enemies;
    use crate::fight::battle::testing::*;
    use serde_json::json;

    #[test]
    fn golden() {
        let (mut checked, mut cut) = (0, 0);
        for (n, row) in states().iter().enumerate() {
            let _env = env(row);
            let (state, r) = (&row["s"], &row["r"]);
            let so_far = pace(state);
            assert_eq!(json!(so_far), r["pace"], "state {n}");
            let (es, hp) = (read_enemies(state), state["player"]["hp"].int());
            let got: Vec<f64> = [(if so_far != 0.0 { so_far } else { 30.0 }, 5), (200.0, 0), (10.0, hp)].iter().map(|(p, lost)| discount(*p, *lost, hp, &es)).collect();
            assert_eq!(json!(got), r["discount"], "state {n}");
            checked += 4;
        }
        let g: Value = serde_json::from_str(include_str!("../../tests/golden/fight/closing.json")).unwrap();
        for rec in calls(&g, "closing.discount", 350) {
            let _env = env(&rec["env"]);
            let a = &rec["a"];
            let (all, got) = (foes(&a[3]), discount(a[0].float(), a[1].int(), a[2].int(), &foes(&a[3])));
            assert_eq!(json!(got), rec["r"], "discount {a}");
            assert_eq!(discount(a[0].float(), a[1].int(), a[2].int(), &all.iter().collect::<Vec<_>>()), got);
            cut += (got != 1.0) as usize;
            checked += 1;
        }
        assert!(checked > 1500 && cut > 15, "{checked} checks, {cut} discounts");
        {
            let _off = CLOSE_REACH.set(0.0);
            let boss = [Enemy { hp: 10, ..Enemy::default() }];
            assert_eq!(discount(30.0, 0, 50, &boss), 1.0);
        }
        assert_eq!(discount(30.0, 0, 50, &[Enemy { hp: 10, ..Enemy::default() }]), 0.4);
    }
}
