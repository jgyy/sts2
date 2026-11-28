//! One log line per boss fight, written when it is over (a won fight leaves no "run over" line in the floor log).

use std::collections::BTreeSet;

use serde_json::Value;

use super::combat;
use crate::base::{self, JsonExt};

/// Screens the game opens in the middle of a fight (a discard, a choice): they do not end it.
pub const _IN_FIGHT_SCREENS: [&str; 4] = ["hand_select", "card_select", "bundle_select", "overlay"];

/// A screen that opens inside a fight (the first game's card choices from a potion or a card are a card reward `in_combat`).
pub fn mid_fight(state: &Value, kind: Option<&str>) -> bool {
    kind.is_some_and(|k| _IN_FIGHT_SCREENS.contains(&k)) || (kind == Some("card_reward") && state["card_reward"]["in_combat"].truthy())
}

pub fn _base_name(entity_id: &str) -> String {
    // (the original's `$` also stands before a last line break)
    crate::re!(r"_\d+(\n?)$").replace(entity_id, "${1}").into_owned()
}

/// `str(x.get(field) or x.get("name") or "?")` for each thing the player holds under `key`.
fn _held(player: &Value, key: &str, field: &str) -> Vec<String> {
    player[key].list().iter().map(|x| base::str_of(x[field].or(&x["name"]).or(&Value::String("?".into())))).collect()
}

/// What `BossWatch` knows of the fight it is watching (the original's `fight` dict).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct BossFight {
    /// (act, floor) as the run gives them
    pub r#where: (Value, Value),
    pub names: Vec<String>,
    pub boss_hp0: i64,
    pub hp0: i64,
    pub max_hp: i64,
    pub rounds: BTreeSet<i64>,
    // What the bot brought to the fight, to tell later what wins them.
    pub relics: i64,
    pub potions: i64,
    pub deck: i64,
    pub boss_hp: Option<i64>,
    pub hp: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct BossWatch {
    pub fight: Option<BossFight>,
    /// a log line for the fight that has just begun: what the bot holds going in
    pub brought: Option<String>,
}

impl BossWatch {
    pub fn new() -> Self {
        BossWatch { fight: None, brought: None }
    }

    /// Feed every state the bot reads. Returns a log line when a boss fight has just ended.
    pub fn update(&mut self, state: &Value, kind: Option<&str>) -> Option<String> {
        if kind == Some("boss") && state["battle"].truthy() {
            self._seen(state);
            return None;
        }
        if self.fight.is_none() || mid_fight(state, kind) {
            return None;
        }
        let fight = self.fight.take()?;
        Some(Self::_summary(&fight, kind == Some("game_over") && !state["game_over"]["victory"].truthy()))
    }

    pub fn _seen(&mut self, state: &Value) {
        let battle = &state["battle"];
        let player = &state["player"];
        let enemies = combat::read_enemies(state);
        if enemies.is_empty() && self.fight.is_none() {
            return; // nothing alive yet: not a fight we can report on
        }
        let boss_hp: i64 = enemies.iter().map(|e| e.hp).sum();
        let run = &state["run"];
        if self.fight.is_none() {
            let fight = BossFight {
                r#where: (run["act"].clone(), run["floor"].clone()),
                names: enemies.iter().map(|e| _base_name(&e.entity_id)).collect::<BTreeSet<_>>().into_iter().collect(),
                boss_hp0: boss_hp,
                hp0: player["hp"].int(),
                max_hp: player["max_hp"].int(),
                rounds: BTreeSet::new(),
                // What the bot brought to the fight, to tell later what wins them.
                relics: player["relics"].list().len() as i64,
                potions: player["potions"].list().len() as i64,
                deck: ["hand", "draw_pile", "discard_pile"].iter().map(|pile| player[*pile].list().len() as i64).sum(),
                boss_hp: None,
                hp: None,
            };
            // ... and by name: the run history only has the deck a run ended with.
            let mut cards: Vec<String> = ["hand", "draw_pile", "discard_pile"].iter().flat_map(|pile| _held(player, pile, "name")).collect();
            cards.sort();
            let who = fight.names.join("+");
            self.brought = Some(format!(
                "boss fight begins: act {} floor {} {}; relics {}; potions {}; cards {}",
                base::str_of(&run["act"]),
                base::str_of(&run["floor"]),
                if who.is_empty() { "?" } else { &who },
                _held(player, "relics", "id").join(" "),
                _held(player, "potions", "name").join(", "),
                cards.join(", ")
            ));
            self.fight = Some(fight);
        }
        let f = self.fight.as_mut().expect("a fight is being watched");
        f.boss_hp = Some(boss_hp);
        f.hp = Some(player["hp"].int());
        let mut names: BTreeSet<String> = f.names.iter().cloned().collect();
        names.extend(enemies.iter().map(|e| _base_name(&e.entity_id)));
        f.names = names.into_iter().collect();
        if !battle["round"].is_null() {
            f.rounds.insert(battle["round"].int());
        }
    }

    pub fn _summary(f: &BossFight, lost: bool) -> String {
        let (act, floor) = &f.r#where;
        let who = f.names.join("+");
        format!(
            "boss fight over: act {} floor {} {} {}; boss hp {} -> {}; player hp {} -> {}/{}; rounds {}; brought relics {}, potions {}, deck {}",
            base::str_of(act),
            base::str_of(floor),
            if who.is_empty() { "?" } else { &who },
            if lost { "lost" } else { "won" },
            f.boss_hp0,
            f.boss_hp.unwrap_or(f.boss_hp0),
            f.hp0,
            f.hp.unwrap_or(f.hp0),
            f.max_hp,
            f.rounds.len(),
            f.relics,
            f.potions,
            f.deck
        )
    }
}

/// What the golden tests of the two watches (here and in src/fightrecord.rs) share: the sequences of tests/golden/fight2, read back.
#[cfg(test)]
pub(crate) mod testing {
    use serde_json::Value;

    use crate::fight::combat::testing::{rows, Golden};
    use crate::base::JsonExt;

    /// `v` with every `{"@": n}` in it replaced by the n-th atom.
    pub fn unpack(v: &Value, atoms: &[Value]) -> Value {
        match v {
            Value::Object(m) if m.len() == 1 && m.contains_key("@") => atoms[m["@"].int() as usize].clone(),
            Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), unpack(x, atoms))).collect()),
            Value::Array(a) => Value::Array(a.iter().map(|x| unpack(x, atoms)).collect()),
            other => other.clone(),
        }
    }

    /// The settings table and every sequence: its env, its calls `(state, kind)`, what the original watches answered each, the lines written.
    pub fn sequences() -> (Golden, Vec<Value>) {
        let shared = include_str!("../../tests/golden/fight2/watch_atoms.json");
        let atoms = rows(&[shared], "atoms");
        let knobs = rows(&[shared], "knobs").iter().map(|k| k.entries().map(|(name, x)| (name.clone(), x.float())).collect()).collect();
        let mut seqs = rows(&[include_str!("../../tests/golden/fight2/watch0.json"), include_str!("../../tests/golden/fight2/watch1.json")], "watch");
        for seq in seqs.iter_mut() {
            seq["calls"] = unpack(&seq["calls"], &atoms);
        }
        (Golden { atoms: Vec::new(), cards: Vec::new(), knobs }, seqs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fight::combat::testing::rows;
    use serde_json::json;

    #[test]
    fn golden_small() {
        let text = include_str!("../../tests/golden/fight2/small.json");
        let (mid, base) = (rows(&[text], "mid"), rows(&[text], "base"));
        assert!(mid.len() >= 7 && base.len() >= 40);
        for rec in &mid {
            assert_eq!(json!(mid_fight(&rec["s"], rec["kind"].as_str())), rec["r"], "mid_fight {rec}");
        }
        for rec in &base {
            assert_eq!(json!(_base_name(rec["id"].s())), rec["r"], "_base_name {rec}");
        }
        assert_eq!((_base_name("A_1\n"), _base_name("A_B"), _base_name("X_12_3")), ("A\n".to_string(), "A_B".to_string(), "X_12".to_string()));
        assert!(mid_fight(&json!({}), Some("overlay")) && !mid_fight(&json!({"card_reward": {"in_combat": false}}), Some("card_reward")) && !mid_fight(&json!({}), None));
    }

    #[test]
    fn golden_boss_watch() {
        let (g, seqs) = testing::sequences();
        assert!(seqs.len() >= 45);
        let (mut calls, mut ended, mut begun) = (0, 0, 0);
        for (n, seq) in seqs.iter().enumerate() {
            let _env = g.env(&seq["env"]);
            let mut watch = BossWatch::new();
            for (i, call) in seq["calls"].list().iter().enumerate() {
                let said = watch.update(&call[0], call[1].as_str());
                // (as the bot does: the line is logged and forgotten)
                let brought = watch.brought.take();
                let want = &seq["r"][i];
                assert_eq!((json!(said), json!(brought)), (want[0].clone(), want[1].clone()), "sequence {n} ({}), call {i}: {}", seq["src"], call[1]);
                calls += 1;
                ended += said.is_some() as usize;
                begun += brought.is_some() as usize;
            }
        }
        assert!(calls > 4000 && ended > 40 && begun > 40, "{calls} calls, {ended} fights over, {begun} begun");
    }
}
