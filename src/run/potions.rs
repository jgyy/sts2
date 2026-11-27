//! Potions worth drinking in a fight, and the one-line picture of a fight for the debug log.

use std::collections::HashSet;
use std::sync::LazyLock;

use indexmap::IndexMap;
use regex::Regex;
use serde_json::{json, Value};

use crate::cards::{parse, CardInfo};
use crate::fight::battle::{end_of_turn_block, expected_loss, osty_hp, read_enemies};
use crate::games;
use crate::games::sts1::stance::read as read_stance;
use crate::learn::knob;
use crate::py::{self, JsonExt};
use crate::Action;

pub const BOSS_FLOORS: [i64; 3] = [17, 33, 48];
pub const STS1_BOSS_FLOORS: [i64; 4] = [16, 33, 50, 54]; // the first game's acts are 17 floors each, the boss on the last; act 4: rest, shop, elite, the Heart

/// The floors each act's boss is met on, in the game being played.
pub fn boss_floors() -> &'static [i64] {
    if games::uses_sts2_tables() {
        &BOSS_FLOORS
    } else {
        &STS1_BOSS_FLOORS
    }
}

/// Floors before each act's boss from which a full belt is no longer a reason to drink (a slot emptied there seldom fills in time).
pub const SAVE_FLOORS: [(i64, i64); 4] = [(1, 6), (2, 9), (3, 15), (4, 3)]; // (act 4: the Spire Shield and Spear's potions are kept for the Heart unless in danger)
pub const SPARE_BEYOND: i64 = 3; // potions a boss fight is kept: more than that on the belt are drunk on the way

// (Dexterity for the whole fight, the Dexterity Potion's 2, is not one of these: it goes early, like Strength.)
pub fn _DEFENSIVE_POTION() -> &'static Regex {
    crate::re!(r"(?i)gain \d+ block|gain \d+ dexterity\. at the end of your turn|apply \d+ weak|enem(y|ies) loses? \d+ strength")
}

/// Strength for one turn (Flex Potion: "Gain 5 Strength. At the end of your turn, lose 5 Strength."): its own "lose 5 Strength" is no defence.
pub fn _ONE_TURN_STRENGTH() -> &'static Regex {
    crate::re!(r"(?i)gain \d+ strength\. at the end of your turn, lose")
}

pub fn _ENEMY_DEBUFF() -> &'static Regex {
    crate::re!(r"(?i)apply \d+ (weak|vulnerable|poison|doom)|enem(y|ies) loses? \d+ (strength|hp at the end)|enemy's attacks deal \d+% less")
}

/// What a potion is worth in the boss fight it is carried to, in damage or Block there (the least is drunk on the way; others priced from text).
#[rustfmt::skip]
pub static BOSS_WORTH: LazyLock<IndexMap<&'static str, i64>> = LazyLock::new(|| {
    IndexMap::from([
        ("fairy in a bottle", 60), ("mazaleth's gift", 50), ("strength potion", 45), ("focus potion", 40), ("fysh oil", 38), ("powdered demise", 36), ("potion of doom", 33), ("radiant tincture", 32), ("king's courage", 30), ("vulnerable potion", 30),
        ("dexterity potion", 30), ("orobic acid", 28), ("power potion", 26), ("heart of iron", 25), ("gigantification potion", 25), ("soldier's stew", 24), ("distilled chaos", 24), ("beetle juice", 24), ("liquid bronze", 22), ("blood potion", 20),
        ("fire potion", 20), ("poison potion", 20), ("ship in a bottle", 20), ("touch of insanity", 20), ("lucky tonic", 18), ("cunning potion", 18), ("energy potion", 16), ("clarity extract", 16), ("regen potion", 15), ("weak potion", 15),
        ("potion of binding", 15), ("bone brew", 15), ("blessing of the forge", 15), ("flex potion", 15), ("potion-shaped rock", 15), ("cure all", 14), ("block potion", 12), ("attack potion", 12), ("duplicator", 12), ("shackling potion", 12), ("star potion", 12),
        ("explosive ampoule", 10), ("skill potion", 10), ("colorless potion", 10), ("liquid memories", 10), ("speed potion", 10), ("snecko oil", 10), ("swift potion", 8), ("bottled potential", 8), ("glowwater potion", 8), ("pot of ghouls", 8), ("fortifier", 8),
        ("droplet of precognition", 6), ("gambler's brew", 5), ("stable serum", 5), ("foul potion", 4),
    ])
});

pub fn _FILLS_BELT() -> &'static Regex {
    crate::re!(r"(?i)fill all (?:your )?empty potion slots")
}

/// What this potion (one on the belt, or a reward or shop item) is worth kept for the next boss.
pub fn boss_worth(potion: &Value) -> f64 {
    let name = potion["name"].or(&potion["potion_name"]).text().trim().to_lowercase();
    if let Some(worth) = BOSS_WORTH.get(name.as_str()) {
        return *worth as f64;
    }
    let text = potion["description"].or(&potion["potion_description"]).text();
    if _FILLS_BELT().is_match(&text) || crate::re!(r"(?i)gain \d+ max hp").is_match(&text) {
        return 0.0; // (drunk at the first chance: it is not for a boss)
    }
    if let Some(m) = crate::re!(r"(?i)deals? (\d+) damage").captures(&text) {
        return m[1].parse::<f64>().unwrap_or(0.0);
    }
    if crate::re!(r"(?i)\b(strength|dexterity|focus|ritual|plating)\b").is_match(&text) && !text.to_lowercase().contains("at the end of your turn") {
        22.0
    } else {
        12.0
    }
}

/// The next boss is near enough that what is on the belt is kept for it (see SAVE_FLOORS).
pub fn saving(floor: &Value) -> bool {
    let floor = match floor {
        Value::Bool(b) => *b as i64, // (`isinstance(True, int)`)
        other if other.is_int() => other.int(),
        _ => return false,
    };
    boss_floors().iter().zip(SAVE_FLOORS.iter()).any(|(boss, (_act, save))| 0 < boss - floor && boss - floor <= *save)
}

/// `p.get("can_use_in_combat", True)`.
fn usable(p: &Value) -> bool {
    p.get("can_use_in_combat").is_none_or(|x| x.truthy())
}

/// A potion that triples our Block is on the belt (the planner then counts on it when nothing else saves the turn).
pub fn fortifier(player: &Value) -> bool {
    player["potions"].list().iter().any(|p| usable(p) && crate::re!(r"(?i)triple your block").is_match(&p["description"].text()))
}

/// A potion that does nothing, or next to nothing, drunk at this moment (most went on round 1 of a boss fight, whatever
/// they said). `line` is what the planner means to play this turn (None: not known). (The Python's default: losing 0.)
pub fn _ill_timed(text: &str, player: &Value, line: Option<&[CardInfo]>, block: i64, incoming: i64, danger: bool, losing: i64) -> bool {
    let hand: Vec<CardInfo> = player["hand"].list().iter().map(parse).collect();
    let cards: &[CardInfo] = match line {
        None => &hand,
        Some(line) => line,
    };
    if crate::re!(r"(?i)triple your block").is_match(text) {
        // ... once the turn's Block is up (the planner has nothing left to play) and still short of what is coming.
        return !(line.is_some_and(|l| l.is_empty()) && block >= 8 && incoming > block);
    }
    if crate::re!(r"(?i)from your discard pile").is_match(text) {
        return player["discard_pile_count"].int() < 3;
    }
    if crate::re!(r"(?i)exhaust your hand").is_match(text) {
        // (Glowwater: "Exhaust your Hand. Draw 10 cards": for a hand of Wounds and Burns, or energy enough for ten cards.)
        return !danger && hand.iter().filter(|c| c.r#type == "Status" || c.r#type == "Curse").count() < 2 && player["energy"].int() < 5;
    }
    if _ONE_TURN_STRENGTH().is_match(text) {
        return cards.iter().filter(|c| c.r#type == "Attack" && c.damage != 0).map(|c| c.hits).sum::<i64>() < (if danger { 1 } else { 3 });
    }
    if crate::re!(r"(?i)gain \d+ dexterity\. at the end of your turn").is_match(text) {
        return (cards.iter().filter(|c| c.block != 0).count() as i64) < (if danger { 1 } else { 2 }); // (Speed Potion: 5 more on each Block card played this turn)
    }
    let icons = crate::re!(r"energy_icon").find_iter(text).count() as i64;
    if icons != 0 && crate::re!(r"(?i)^(?:gain (?:\[\w*energy_icon\.png\])+\.?)$").is_match(text.trim()) {
        // Energy Potion: for a turn with cards left to spend it on (an attack, Power, draw or X card; Block only if the plan still loses HP).
        let planned: HashSet<i64> = line.unwrap_or(&[]).iter().map(|c| c.index).collect();
        let spare = hand.iter().filter(|c| !planned.contains(&c.index) && c.r#type != "Status" && c.r#type != "Curse" && (c.playable || c.short));
        let wanted = spare.filter(|c| c.is_x() || (c.cost > 0 && (c.damage != 0 || c.r#type == "Power" || c.draw != 0 || (c.block != 0 && losing > 0))));
        return wanted.map(|c| if c.is_x() { icons } else { c.cost }).sum::<i64>() < icons;
    }
    if crate::re!(r"(?i)next card is played an extra time").is_match(text) {
        return !cards.iter().any(|c| c.r#type == "Power" || c.cost >= 2); // (Duplicator: 9 of 11 doubled whatever came first)
    }
    false
}

/// Potions worth drinking now: always in elite/boss fights, otherwise when in danger. `planned_loss` is what the planner
/// expects to lose this turn after playing the best cards in hand (None: not known), `line` the cards it means to play.
pub fn potion_actions(state: &Value, planned_loss: Option<i64>, line: Option<&[CardInfo]>) -> Vec<Action> {
    let player = &state["player"];
    let enemies = read_enemies(state);
    if enemies.is_empty() {
        return vec![];
    }
    let hp = if player["hp"].truthy() { player["hp"].int() } else { 1 };
    let max_hp = if player["max_hp"].truthy() { player["max_hp"].int() } else { 1 };
    // Ordinary fights only get a potion in real trouble; the rest are kept for elites and bosses, where runs are lost.
    let mut danger = expected_loss(state) as f64 >= 0.5 * hp as f64 || hp as f64 <= 0.3 * max_hp as f64;
    if let Some(planned_loss) = planned_loss {
        // Judged by what still gets through after the best cards in hand (the plan is redone after every potion).
        danger = planned_loss as f64 >= 0.5 * hp as f64 || (hp as f64 <= 0.3 * max_hp as f64 && planned_loss > 0);
    }
    let mut potions: Vec<&Value> = player["potions"].list().iter().collect();
    let kind = state["state_type"].as_str();
    // A full belt cannot take the next potion a fight drops, so one of them is free to use; in a hallway fight only while the
    // `belt_spare` setting says so (0: kept for an elite or the boss, a drop taken only if worth more there). Model runs, 2026-11-26:
    // Slay the Spire 2 at A8 (two slots) 1.2 floors further with 0, the act 2 boss won 63% of the time, not 54%; the first game at
    // A0 (three slots, act 2 and 3 hallways that end runs) 1.5 floors short with it (learn.GAME_DEFAULTS).
    let slots = if player["max_potion_slots"].truthy() { player["max_potion_slots"].int() } else { 3 };
    let mut spare: i64 = if potions.len() as i64 >= slots && (kind != Some("monster") || knob("belt_spare") != 0.0) { 1 } else { 0 };
    let floor = &state["run"]["floor"];
    if matches!(kind, Some("monster") | Some("elite")) && saving(floor) {
        spare = 0; // the boss is a few rooms away: what is on the belt goes there
    } else if matches!(kind, Some("monster") | Some("elite")) {
        spare = spare.max(potions.len() as i64 - SPARE_BEYOND); // a belt of more than three is never "full": what is beyond is for the way
    }
    // Runs end at bosses, not elites: an elite gets the spare one, and the rest only once the fight has gone wrong.
    let hurt = kind == Some("elite") && hp as f64 <= 0.5 * max_hp as f64;
    let mut big_fight = danger || hurt || kind == Some("boss");
    // Delicate Frond fills the belt again at the start of every fight: nothing is being saved.
    if player["relics"].list().iter().any(|r| r["description"].text().to_lowercase().contains("fill all empty potion slots")) {
        big_fight = true;
        spare = potions.len() as i64;
    }
    // What a potion gives for good (Fruit Juice's Max HP, Entropic Brew's potions) is worth the same in any fight: at the first chance.
    let for_good = |p: &Value| {
        let text = p["description"].text();
        crate::re!(r"(?i)gain \d+ max hp").is_match(&text) || _FILLS_BELT().is_match(&text)
    };
    let slot_of = |p: &Value| p.get("slot").map(|s| s.int()).unwrap_or(0);
    let keeps: Vec<Action> = potions.iter().filter(|p| usable(p) && for_good(p)).map(|p| ("use_potion".to_string(), json!({"slot": slot_of(p)}))).collect();
    if !keeps.is_empty() {
        return keeps;
    }
    if kind != Some("boss") {
        py::sort_by_key(&mut potions, |p| boss_worth(p)); // what is drunk on the way to a boss is what would do the least there
    }
    if !(big_fight || spare != 0) {
        return vec![];
    }
    if enemies.iter().any(|e| e.asleep >= 2) && !danger {
        return vec![]; // asleep: until the turn before she wakes we only set up, and a potion drunk now is poured out
    }
    let left: i64 = enemies.iter().filter(|e| e.worth() > 0.0).map(|e| e.hp + e.block).sum();
    if left <= 0 || (left <= 25 && !danger) {
        return vec![]; // nothing left to hit (a boss already beaten), or the fight is as good as over
    }
    let block = player["block"].int();
    // Aim at something that stays dead and matters: the leader that hits hardest.
    let focus = py::max_by_key(enemies.iter(), |e| (e.worth(), e.incoming, e.hp)).unwrap().entity_id.clone();
    let mut out: Vec<Action> = Vec::new();
    let slippery = enemies.iter().any(|e| e.slippery != 0 || e.hit_cap == 1); // (Intangible: the same, for a turn)
    // What is thrown at the Test Subject's first form is gone when it revives, and at a sleeper only wakes it: those potions wait.
    let passing = enemies.iter().any(|e| (e.adapts && e.enrage != 0) || e.asleep >= 2);
    let shielded = enemies.iter().find(|e| e.entity_id == focus).map(|e| e.artifact > 0).unwrap();
    let incoming: i64 = enemies.iter().map(|e| e.incoming).sum();
    for p in potions {
        if !usable(p) {
            continue;
        }
        if !big_fight && out.len() as i64 >= spare {
            break;
        }
        let text = p["description"].text();
        let target_type = py::str_of(&p["target_type"]);
        // Every hit on a Slippery enemy takes 1 HP: keep damage, and what makes our hits bigger, for later.
        if slippery && crate::re!(r"(?i)deals? \d+ damage|\bstrength\b|\bvigor\b|\bvulnerable\b|\bpoison\b|\bdoom\b").is_match(&text) {
            continue;
        }
        if passing && !danger && (target_type == "AnyEnemy" || target_type == "AllEnemies" || crate::re!(r"(?i)enem(y|ies)").is_match(&text)) {
            continue;
        }
        // Artifact swallows what a potion would put on the enemy: wait until the cards have worn it off.
        if shielded && !danger && _ENEMY_DEBUFF().is_match(&text) {
            continue;
        }
        if let Some(both) = crate::re!(r"(?i)deals? (\d+) damage to all (?:players|characters)").captures(&text) {
            // Foul Potion hits us as hard as them: only behind the Block left after this turn's attacks, or to end the fight.
            let amount: i64 = both[1].parse().unwrap_or(0);
            let spare_block = block + end_of_turn_block(player) - incoming;
            if !(spare_block >= amount || (hp > amount && enemies.iter().all(|e| e.hp + e.block <= amount))) {
                continue;
            }
        }
        if crate::re!(r"(?i)\bheal\b|\bregen\b").is_match(&text) && hp as f64 > 0.55 * max_hp as f64 {
            continue; // healing is wasted on a full bar; it is for later in the fight
        }
        if _ill_timed(&text, player, line, block, incoming, danger, planned_loss.unwrap_or(0)) {
            continue;
        }
        if _DEFENSIVE_POTION().is_match(&text) && expected_loss(state) < 12.min(hp) {
            continue; // Block, Weak and the like are for the turn something big is coming
        }
        let mut params = json!({"slot": slot_of(p)});
        if target_type == "AnyEnemy" {
            params["target"] = json!(&*focus);
        }
        out.push(("use_potion".to_string(), params));
    }
    out
}

/// One line with everything the planner looked at (for the debug log).
pub fn describe(state: &Value) -> String {
    let player = &state["player"];
    let battle = &state["battle"];
    let shown = |v: &Value, key: &str| py::str_of(&v[key]); // f"{d.get(key)}"
    let powers_of = |c: &Value| {
        let one = |s: &Value| {
            let id = match s.get("id") {
                Some(id) => py::str_of(id),
                None => String::new(),
            };
            format!("{}:{}", id.replace("_POWER", ""), shown(s, "amount"))
        };
        c["status"].list().iter().map(one).collect::<Vec<_>>().join(",")
    };
    let mut foes = Vec::new();
    for e in battle["enemies"].list() {
        let powers = powers_of(e);
        let intents = e["intents"].list().iter().map(|i| format!("{}{}", shown(i, "type"), if i["label"].truthy() { py::str_of(&i["label"]) } else { String::new() })).collect::<Vec<_>>().join("+");
        foes.push(format!("{} {}hp/{}blk [{}] {}", shown(e, "entity_id"), shown(e, "hp"), shown(e, "block"), powers, intents));
    }
    /// What the planner makes of the card, e.g. " d6x2 b5" (so a misread shows in the log).
    fn read(c: &Value) -> String {
        let i = parse(c);
        let hits = i.hits * (1 + i.replay); // (a card that plays itself again shows as that many hits)
        let mut bits: Vec<String> = if i.damage != 0 { vec![format!("d{}", i.damage) + &(if hits > 1 { format!("x{hits}") } else { String::new() })] } else { vec![] };
        if i.block != 0 {
            bits.push(format!("b{}", i.block));
        }
        if !bits.is_empty() {
            format!(" {}", bits.join(" "))
        } else {
            String::new()
        }
    }

    let hand = player["hand"]
        .list()
        .iter()
        .map(|c| format!("{}:{}({}{}{})", shown(c, "index"), shown(c, "name"), shown(c, "cost"), if c.get("can_play").is_none_or(|x| x.truthy()) { "" } else { "!" }, read(c)))
        .collect::<Vec<_>>()
        .join(", ");
    let mine = powers_of(player);
    let mut extra = String::new();
    if !player["stars"].is_null() {
        extra += &format!(" stars {}", shown(player, "stars"));
    }
    if !player["orbs"].is_null() {
        let orbs: String = player["orbs"].list().iter().map(|o| (if o["name"].truthy() { py::str_of(&o["name"]) } else { "?".to_string() }).chars().next().unwrap().to_string()).collect();
        extra += " orbs ";
        extra += if orbs.is_empty() { "-" } else { &orbs };
    }
    if player["pets"].truthy() {
        extra += &format!(" osty {}", osty_hp(player));
    }
    if !read_stance(&player["stance"]).is_empty() {
        extra += &format!(" stance {}", read_stance(&player["stance"])); // (the Watcher's: intents and card numbers already show it)
    }
    format!(
        "r{} hp {}/{} blk {} en {}{} [{}] | {} | hand {}",
        shown(battle, "round"),
        shown(player, "hp"),
        shown(player, "max_hp"),
        shown(player, "block"),
        shown(player, "energy"),
        extra,
        mine,
        foes.join(" ; "),
        hand
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fight::combat::testing::*;

    #[test]
    fn golden_potion_actions() {
        let g = golden();
        let cases = rows(&[CALLS], "potion");
        assert!(cases.len() >= 300, "{} cases", cases.len());
        let mut drunk = 0;
        for (n, rec) in cases.iter().enumerate() {
            let _env = g.env(&rec["env"]);
            let state = g.state(&rec["s"]);
            let line = g.line(&rec["line"]);
            let got = potion_actions(&state, rec["loss"].as_i64(), line.as_deref());
            drunk += got.len();
            assert_eq!(actions_json(&got), rec["r"], "case {n}: {}", rec["s"]);
        }
        assert!(drunk > 100, "{drunk} potions drunk");
    }

    #[test]
    fn golden_ill_timed() {
        let g = golden();
        let cases = rows(&[CALLS], "ill");
        assert!(cases.len() >= 80, "{} cases", cases.len());
        let mut ill = 0;
        for (n, rec) in cases.iter().enumerate() {
            let _env = g.env(&rec["env"]);
            let line = g.line(&rec["line"]);
            let got = _ill_timed(rec["text"].s(), &g.player(&rec["p"]), line.as_deref(), rec["block"].int(), rec["incoming"].int(), rec["danger"].truthy(), rec["losing"].int());
            ill += got as usize;
            assert_eq!(json!(got), rec["r"], "case {n}: {}", rec["text"]);
        }
        assert!(ill > 10 && ill < cases.len() - 10, "{ill} ill-timed of {}", cases.len());
    }

    #[test]
    fn golden_worth_and_floors() {
        let v: Value = serde_json::from_str(include_str!("../../tests/golden/combat/potions.json")).unwrap();
        for game in ["sts2", "sts1"] {
            let _g = games::playing(game);
            assert_eq!(json!(boss_floors()), v["boss_floors"][game]);
        }
        let (worths, floors) = (v["boss_worth"].list(), v["saving"].list());
        assert!(worths.len() > 600 && floors.len() > 130);
        for rec in worths {
            let _g = games::playing(rec["g"].s());
            assert_eq!(json!(boss_worth(&rec["p"])), rec["r"], "{}", rec["p"]);
        }
        for rec in floors {
            let _g = games::playing(rec["g"].s());
            assert_eq!(json!(saving(&rec["floor"])), rec["r"], "{} floor {}", rec["g"], rec["floor"]);
        }
        assert_eq!((SPARE_BEYOND, SAVE_FLOORS[2], BOSS_WORTH["fortifier"], BOSS_WORTH.len()), (3, (3, 15), 8, 56));
    }
}
