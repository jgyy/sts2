//! Slay the Spire 2's events, one rule each (2026-11-26, from the game's own texts in localization/eng/events.json, the
//! options the bot met in its logs, and what each named card, relic and enchantment does). A rule returns an option's score on
//! the generic scorer's scale (events._event_option_value: a relic 6, an upgrade 4), or None to leave it to that scorer, which
//! reads the text; `COVERED` lists every event and the ones whose generic score was checked and found right.
//!
//! The shape is sts1.rs's: a rule is a plain function `fn(opt: &Value, ctx: &Ctx) -> Option<f64>` ([`Rule`]), named as in
//! the original; `RULES` maps an event's id to its rule (an `IndexMap` in the original's row order); `value(event_id, opt, ctx)` looks the
//! rule up and calls it. (the original's `events.rule_value` catches any exception of a rule and gives None: these rules raise nothing
//! on options, decks and potions made of JSON objects; only a number too long for an i64 in a text would panic, in `value::num`.)

use std::collections::BTreeSet;
use std::sync::LazyLock;

use indexmap::IndexMap;
use serde_json::{json, Value};

pub use super::sts1::Rule;
use super::value::{
    counts, curse, gold, heal, hp_cost, max_hp_gain, max_hp_loss, num, potions_gained, random_upgrades, removals, transforms, Ctx, CARD_REWARD, POTION, RANDOM_CURSE, RELIC,
    UPGRADE,
};
use crate::base::{self, fmax, fmin, JsonExt};
use crate::run::relicworth::{id_of as relic_id, worth as relic_worth};

pub const BASE: f64 = 0.5; // an option's worth before what it gives and costs (as the generic scorer)
/// The chance the next act's map leads us to the War Historian the Lantern Key opens: not known (it never has been kept); at 0.35
/// the key is still returned for its gold, as before. Raise it once a kept key is seen to bring the event.
pub const MEET_WAR_HISTORIAN: f64 = 0.35;

/// `str.strip()`: the original's blanks (those of Unicode and the four separators 0x1c-0x1f).
fn strip(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c))
}

/// `dataclasses.replace(ctx, hp=hp)`.
fn with_hp(ctx: &Ctx, hp: i64) -> Ctx {
    Ctx { hp, ..ctx.clone() }
}

pub fn _text(opt: &Value) -> (String, String) {
    (strip(&opt["title"].text()).to_lowercase(), opt["description"].text())
}

/// A relic by name: its worth beyond a relic's (relicworth.rs), on top of a relic's.
pub fn relic(name: &str, ctx: &Ctx) -> f64 {
    RELIC + relic_worth(&relic_id(name), &ctx.deck, Some(ctx.act), ctx.hp, ctx.max_hp)
}

/// An event's ordinary fight: its rewards less the HP it costs (some 8, 12, 14% of Max HP by act).
pub fn _hallway_fight(ctx: &Ctx) -> f64 {
    let share = match ctx.act {
        1 => 0.08,
        2 => 0.12,
        _ => 0.14,
    };
    CARD_REWARD + 0.4 - hp_cost(base::round(share * ctx.max_hp as f64), ctx)
}

// --------------------------------------------------------------------------------------------------- the rules

pub fn abyssal_baths(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "immerse" || title == "linger" {
        return Some(BASE + max_hp_gain(num(r"gain (\d+) max hp", &text, 0)) - hp_cost(num(r"take (\d+) damage", &text, 0), ctx));
    }
    if title == "abstain" {
        return Some(BASE + heal(num(r"heal (\d+)", &text, 0), ctx));
    }
    if title == "exit baths" {
        Some(BASE)
    } else {
        None
    }
}

pub fn amalgamator(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // Two basics become one better card: Strikes first (the bot's removal order: a Strike is the weakest card), Defends after.
    let (title, _) = _text(opt);
    let c = counts(ctx);
    if title == "combine strikes" {
        return Some(if c["strikes"] >= 2 || ctx.deck.is_empty() { BASE + 5.5 } else { -1.0 });
    }
    if title == "combine defends" {
        return Some(if c["defends"] >= 2 || ctx.deck.is_empty() { BASE + 5.0 } else { -1.0 });
    }
    None
}

pub fn aroma_of_chaos(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, _) = _text(opt);
    if title == "maintain control" {
        return Some(BASE + UPGRADE);
    }
    if title == "let go" {
        return Some(BASE + transforms(ctx, 1)); // (better than an upgrade only with a curse in the deck)
    }
    None
}

/// A dummy that never attacks, killed in 3 turns or nothing: 0 of 22 fights killed the 300 HP one (125 dealt on average,
/// the best 247), 4 of 22 the 150 HP one, all 22 the 75 HP one (2026-11-26).
pub fn battleworn_dummy(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (_title, text) = _text(opt);
    let hp = num(r"fight a (\d+) hp dummy", &text, 0);
    if hp == 0 {
        return None;
    }
    let dealt = three_turns(ctx);
    let mut chance = fmax(0.0, fmin(1.0, (dealt - 0.8 * hp as f64) / (0.4 * hp as f64))); // (0 below 80% of its HP, sure from 120%)
    if hp <= 80 {
        chance = fmax(chance, 0.95); // (the 75 HP one died in every logged fight)
    }
    let low = text.to_lowercase();
    let reward = if low.contains("relic") {
        RELIC
    } else if low.contains("upgrade 2") {
        2.0 * 2.0
    } else {
        potions_gained(1, ctx, POTION)
    };
    Some(BASE + chance * reward)
}

/// The damage the deck deals in three turns of all-out attack (the logged dummy fights: 75-247, 125 on average).
pub fn three_turns(ctx: &Ctx) -> f64 {
    use crate::cards::parse;
    let per_card: Vec<_> = ctx.deck.iter().map(parse).collect();
    if ctx.deck.is_empty() || !per_card.iter().any(|i| i.total_damage() != 0) {
        return 125.0; // (the deck's texts unknown: what the logged fights dealt)
    }
    let dmg = base::fsum(per_card.iter().map(|i| (i.total_damage().max(0) + 3 * i.poison + 4 * i.shivs) as f64 + 0.8 * i.doom as f64));
    let mut energy: i64 = per_card.iter().filter(|i| i.total_damage() > 0).map(|i| i.cost.max(1)).sum();
    if energy == 0 {
        energy = 1;
    }
    let per_energy = dmg / energy as f64;
    let per_card_drawn = dmg / per_card.len() as f64;
    let turn = fmin(5.0 * per_card_drawn * 1.3, 3.0 * per_energy); // (a hand of five, three energy: whichever runs out first)
    3.0 * turn
}

pub fn brain_leech(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "share knowledge" {
        return Some(BASE + 1.6); // (one card of five of our own)
    }
    if title == "rip the leech off" {
        return Some(BASE + 1.0 - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx)); // (a Colorless reward of three)
    }
    None
}

/// The cards each colour gives work without the rest of their deck (Ironclad's Strength, Silent's Shivs and Poison) or need it
/// (Osty, Stars, Arrows, Orbs): three rewards of Necrobinder cards were taken 12 times in 21 for a three-way tie.
#[rustfmt::skip]
pub static _COLOUR: LazyLock<IndexMap<&'static str, (&'static str, f64)>> = LazyLock::new(|| IndexMap::from([
    ("red", ("THE IRONCLAD", 1.0)), ("green", ("THE SILENT", 1.0)), ("blue", ("THE DEFECT", 0.6)), ("orange", ("THE REGENT", 0.3)),
    ("pink", ("THE NECROBINDER", 0.2)), ("yellow", ("THE FOOL", 0.3)),
]));

pub fn colorful_philosophers(opt: &Value, _ctx: &Ctx) -> Option<f64> {
    let (title, _) = _text(opt);
    if let Some(colour) = _COLOUR.get(title.as_str()) {
        return Some(BASE + 3.0 * CARD_REWARD * colour.1);
    }
    if title == "equality" {
        return Some(BASE + 2.5); // (Prismatic Shard: card rewards of every colour, as in the first game)
    }
    None
}

/// Three pages: the gold grows with each step down (35 at the top, more below), the centre holds a relic. Priced as the
/// best way down from here, each step's HP taken off: the top page's "Reach Deeper" had only its 5 HP to show.
pub fn colossal_flower(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    let core = RELIC + 1.5; // Pollinous Core: cards drawn every few turns
    if title == "enter the center" {
        return Some(BASE + core - hp_cost(num(r"lose (\d+) hp", &text, 7), ctx));
    }
    if title == "reach deeper" {
        let step = num(r"lose (\d+) hp", &text, 5);
        let later: &[i64] = if step <= 5 { &[6, 7] } else { &[7] }; // (the HP the steps below this one cost)
        let left = with_hp(ctx, ctx.hp - step);
        let down = core - base::fsum(later.iter().enumerate().map(|(i, x)| hp_cost(*x, &with_hp(ctx, left.hp - later[..i].iter().sum::<i64>()))));
        return Some(BASE + fmax(down, gold(if step > 5 { 135 } else { 75 }, &left)) - hp_cost(step, ctx));
    }
    if title == "extract nectar" {
        return Some(BASE + gold(num(r"gain (\d+) gold", &text, 0), ctx));
    }
    None
}

pub fn crystal_sphere(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // Each divination turned up 7-15 gold and now and then a potion (2026-11-24 logs): small either way, the Debt dearer.
    let (title, text) = _text(opt);
    let divine = 0.35;
    if title == "uncover future" {
        return Some(BASE + gold(-num(r"pay (\d+) gold", &text, 0), ctx) + 3.0 * divine);
    }
    if title == "payment plan" {
        return Some(BASE - curse("debt") + 6.0 * divine);
    }
    None
}

pub fn dense_vegetation(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "rest" {
        let healed = num(r"heal (\d+)", &text, 0);
        return Some(BASE + heal(healed, ctx) + _hallway_fight(&with_hp(ctx, ctx.max_hp.min(ctx.hp + healed))));
    }
    if title == "trudge on" {
        return Some(BASE + gold(num(r"gain (\d+) gold", &text, 0), ctx) - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx));
    }
    if title == "fight!" {
        return Some(BASE);
    }
    None
}

pub fn doll_room(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    let doll = RELIC - 1.0; // (a doll: some do little)
    if title == "pick at random" {
        return Some(BASE + doll);
    }
    if title == "take some time" {
        return Some(BASE + doll + 0.6 - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx));
    }
    if title.starts_with("examine each") {
        return Some(BASE + doll + 1.0 - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx));
    }
    None
}

pub fn doors_of_light_and_dark(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // A tie at 5.5 went to the upgrade 23 times in 23: a removal is the better door with a curse or Strikes in the deck.
    let (title, _) = _text(opt);
    if title == "light door" {
        return Some(BASE + UPGRADE);
    }
    if title == "dark door" {
        return Some(BASE + removals(ctx, 1));
    }
    None
}

pub fn drowning_beacon(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "bottle" {
        return Some(BASE + potions_gained(1, ctx, POTION));
    }
    if title == "climb" {
        return Some(BASE + relic("Fresnel Lens", ctx) - max_hp_loss(num(r"lose (\d+) max hp", &text, 0), ctx));
    }
    None
}

/// A dish of the Endless Conveyor: what it is, before its gold.
pub type Dish = fn(&str, &Ctx) -> f64;

/// Endless Conveyor: what each dish is, before its gold
#[rustfmt::skip]
pub static _BELT: LazyLock<IndexMap<&'static str, Dish>> = LazyLock::new(|| {
    let rows: Vec<(&'static str, Dish)> = vec![
        ("caviar", |text, _ctx| max_hp_gain(num(r"gain (\d+) max hp", text, 5))),
        ("clam roll", |text, ctx| heal(num(r"heal (\d+)", text, 10), ctx)),
        ("fried eel", |_text, _ctx| 0.4), // (a random Colorless card)
        ("golden fysh", |text, ctx| gold(num(r"gain (\d+) gold", text, 0), ctx)),
        ("jelly liver", |_text, ctx| transforms(ctx, 1) - 0.5), // ("Transform a card": ours to choose?)
        ("seapunk salad", |_text, _ctx| -0.3), // (Feeding Frenzy: Strength for a turn, a weak card)
        ("spicy snappy", |_text, ctx| random_upgrades(1, ctx)),
        ("suspicious condiment", |_text, ctx| potions_gained(1, ctx, POTION)),
    ];
    rows.into_iter().collect()
});

pub fn endless_conveyor(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "observe the chef" {
        return Some(BASE + random_upgrades(1, ctx));
    }
    if let Some(m) = crate::re!(r"^grab (.+?) off the belt").captures(&title) {
        let dish = _BELT.get(&m[1])?;
        let price = num(r"pay (\d+) gold", &text, 0);
        let more = if ctx.gold - price >= 40 { 0.4 } else { 0.0 }; // ("Continue feasting!": another dish to look at after it)
        return Some(BASE + dish(&text, ctx) + gold(-price, ctx) + more);
    }
    if title == "leave" || title == "broke" {
        return Some(BASE);
    }
    None
}

pub fn field_of_man_sized_holes(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, _) = _text(opt);
    if title == "enter your hole" {
        return Some(BASE + 2.0); // Perfect Fit: our best card back on top of every shuffle
    }
    if title == "resist" {
        return Some(BASE + removals(ctx, 2) - curse("normality"));
    }
    None
}

pub fn grave_of_the_forgotten(opt: &Value, _ctx: &Ctx) -> Option<f64> {
    let (title, _) = _text(opt);
    if title.starts_with("accept") {
        return Some(BASE + RELIC - 1.0); // Forgotten Soul: 1 damage an Exhaust
    }
    if title == "confront with truth" {
        return Some(BASE - curse("decay") + 1.5); // (Soul's Power: an Exhaust card stays)
    }
    None
}

pub fn hungry_for_mushrooms(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "big mushroom" {
        return Some(BASE + max_hp_gain(num(r"max hp by (\d+)", &text, 20)) - 3.5); // (two cards fewer in every fight's first hand)
    }
    if title == "fragrant mushroom" {
        return Some(BASE + random_upgrades(num(r"upgrade (\d+) random", &text, 2), ctx) - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx));
    }
    None
}

pub fn jungle_maze_adventure(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "join forces" || title == "solo quest" {
        return Some(BASE + gold(num(r"gain (\d+) gold", &text, 0), ctx) - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx));
    }
    None
}

pub fn lost_wisp(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "capture the wisp" {
        return Some(BASE + RELIC - 4.0 + fmin(4.0, 1.0 * counts(ctx)["powers"] as f64) - curse("decay")); // (damage for every Power played)
    }
    if title == "search the nearby area" {
        return Some(BASE + gold(num(r"gain (\d+) gold", &text, 0), ctx));
    }
    None
}

pub fn luminous_choir(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "offer tribute" {
        return Some(BASE + RELIC + gold(-num(r"pay (\d+) gold", &text, 0), ctx));
    }
    if title == "reach into the flesh" {
        return Some(BASE + removals(ctx, 2) - curse("spore mind"));
    }
    None
}

pub fn morphic_grove(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // "Lose ALL of your Gold" was read as the end of the run (-996.5): it is the gold we hold, little some of the time.
    let (title, text) = _text(opt);
    if title == "group" {
        return Some(BASE + transforms(ctx, num(r"transform (\d+) cards", &text, 2)) + gold(-ctx.gold, ctx));
    }
    if title == "loner" {
        return Some(BASE + max_hp_gain(num(r"gain (\d+) max hp", &text, 0)));
    }
    None
}

pub fn museum_of_torture(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "take it" {
        let m = crate::re!(r"(?i)obtain ([a-z' ]+?)\.").captures(&text);
        return Some(BASE + (if let Some(m) = m { relic(&m[1], ctx) } else { RELIC }) - RANDOM_CURSE);
    }
    if title == "break out" {
        return Some(BASE - max_hp_loss(num(r"lose (\d+) max hp", &text, 0), ctx));
    }
    if title == "continue" {
        Some(BASE)
    } else {
        None
    }
}

pub fn round_tea_party(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // Royal Poison takes 4 HP at the start of every fight left: early it is a run's worth, near the end a full heal for little.
    let (title, text) = _text(opt);
    if title == "enjoy your tea" {
        return Some(BASE + heal(ctx.max_hp, ctx) - 4.0 * ctx.fights_left() * 12.0 / ctx.max_hp.max(1) as f64 * 0.5);
    }
    if title == "pick a fight" {
        return Some(BASE + RELIC - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx));
    }
    if title == "continue" {
        Some(BASE)
    } else {
        None
    }
}

pub fn room_full_of_cheese(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "search" {
        return Some(BASE + RELIC - 3.0 + fmin(8.0, 0.35 * ctx.fights_left()) - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx)); // (1 Max HP a fight)
    }
    if title == "gorge" {
        return Some(BASE + 2.0); // (two of eight Commons)
    }
    None
}

pub fn spiraling_whirlpool(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "drink" {
        return Some(BASE + heal(num(r"heal (\d+)", &text, 0), ctx));
    }
    if title == "observe" {
        return Some(BASE + (if counts(ctx)["basics"] != 0 || ctx.deck.is_empty() { 3.0 } else { 0.0 })); // Spiral: a Strike or Defend played twice
    }
    if title == "reach in" {
        return Some(BASE + RELIC - curse("writhe"));
    }
    None
}

pub fn spirit_grafter(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "let it in" {
        return Some(BASE + heal(num(r"heal (\d+)", &text, 0), ctx) + 1.0); // (Metamorphosis: three free Attacks for 2 energy)
    }
    if title == "rejection" {
        return Some(BASE + UPGRADE - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx));
    }
    None
}

pub fn stone_of_all_time(opt: &Value, ctx: &Ctx) -> Option<f64> {
    use crate::run::potions::boss_worth;
    let (title, text) = _text(opt);
    if title == "drink and lift" {
        let m = crate::re!(r"(?i)lose ([a-z' -]+?)\.").captures(&text);
        let named = m.as_ref().map(|m| m[1].to_string());
        let held = match ctx.potions.iter().find(|p| named.as_ref().is_some_and(|n| p["name"].text().to_lowercase() == n.to_lowercase())) {
            Some(p) => p.clone(),
            None => json!({"name": named.as_deref().unwrap_or("")}),
        };
        return Some(BASE + max_hp_gain(num(r"gain (\d+) max hp", &text, 0)) - (1.0 + boss_worth(&held) / 12.0));
    }
    if title == "push" {
        return Some(BASE + 2.0 - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx)); // Vigorous 8: 8 more on an Attack's first play
    }
    None
}

pub fn sunken_statue(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // The Sword of Stone wants 5 Elites after it is taken: 6 runs in 298 fought 5 in the whole run.
    let (title, text) = _text(opt);
    if title == "grab the sword" {
        return Some(BASE);
    }
    if title == "dive into the water" {
        return Some(BASE + gold(num(r"gain (\d+) gold", &text, 0), ctx) - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx));
    }
    None
}

pub fn sunken_treasury(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "first chest" || title == "second chest" {
        return Some(BASE + gold(num(r"gain (\d+) gold", &text, 0), ctx) - (if text.to_lowercase().contains("greed") { curse("greed") } else { 0.0 }));
    }
    None
}

pub fn symbiote(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, _) = _text(opt);
    if title == "approach" {
        return Some(BASE + (if counts(ctx)["big_attacks"] != 0 { 2.5 } else { 1.0 })); // Corrupted: half again the damage of our best Attack, 2 HP a play
    }
    if title == "kill with fire" {
        return Some(BASE + transforms(ctx, 1));
    }
    None
}

pub fn tablet_of_truth(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "smash" {
        return Some(BASE + heal(num(r"heal (\d+)", &text, 0), ctx));
    }
    if title == "give up" {
        return Some(BASE);
    }
    if text.to_lowercase().contains("upgrade all") {
        return Some(BASE + fmin(12.0, 0.8 * ctx.deck.len() as f64) - max_hp_loss(num(r"lose (\d+) max hp", &text, 0), ctx));
    }
    if title.contains("decipher") {
        return Some(BASE + random_upgrades(1, ctx) - max_hp_loss(num(r"lose (\d+) max hp", &text, 0), ctx));
    }
    None
}

/// Keep the Key: a fight (about 27 HP), then the Key (an Unplayable card) opens the War Historian's event in the next act: two
/// relics and two potions, or History Course. Nothing in act 3, which has no next act. It had been returned 16 times in 16.
pub fn the_lantern_key(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "return the key" {
        return Some(BASE + gold(num(r"gain (\d+) gold", &text, 0), ctx));
    }
    if title == "keep the key" {
        let prize = if ctx.act < 3 { MEET_WAR_HISTORIAN * (2.0 * RELIC + 2.0 * POTION) } else { 0.0 }; // (if the event is met)
        return Some(BASE + prize - hp_cost(27, ctx) - 1.0);
    }
    if title == "fight" {
        Some(BASE)
    } else {
        None
    }
}

pub fn the_legends_were_true(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // The Spoils Map: an Unplayable card that marks 600 Gold on the next act's map (the route walks to it: strategy.SPOILS).
    let (title, text) = _text(opt);
    if title == "nab the map" {
        return Some(BASE + (if ctx.act < 3 { 0.8 * gold(600, ctx) } else { 0.0 }) - 1.0);
    }
    if title == "slowly find an exit" {
        return Some(BASE + potions_gained(1, ctx, POTION) - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx));
    }
    None
}

pub fn this_or_that(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "that" {
        let m = crate::re!(r"(?i)add ([a-z' ]+?) to your deck").captures(&text);
        return Some(BASE + RELIC - curse(m.as_ref().map_or("clumsy", |m| m.get(1).map_or("", |g| g.as_str()))));
    }
    if title == "this" {
        return Some(BASE + gold(num(r"gain (\d+) gold", &text, 0), ctx) - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx));
    }
    None
}

#[rustfmt::skip]
pub static _RIDER: LazyLock<IndexMap<&'static str, f64>> = LazyLock::new(|| IndexMap::from([
    ("energized", 3.0), ("wisdom", 2.5), ("violence", 2.5), ("choking", 2.2), ("expertise", 2.0), ("sapping", 1.8), ("improvement", 1.5), ("chaos", 1.0), ("curious", 0.8),
]));

pub fn tinker_time(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, _) = _text(opt);
    if let Some(rider) = _RIDER.get(title.as_str()) {
        return Some(BASE + rider);
    }
    if title == "weapon" || title == "protector" || title == "gadget" {
        let c = counts(ctx);
        let short_of_block = c["blocks"] < base::floordiv(c["attacks"], 2).max(3);
        return Some(
            BASE + match title.as_str() {
                "weapon" => {
                    if short_of_block {
                        1.0
                    } else {
                        1.5
                    }
                }
                "protector" => {
                    if short_of_block {
                        1.5
                    } else {
                        1.0
                    }
                }
                _ => 0.6,
            },
        );
    }
    if title == "accept" {
        Some(BASE + 1.0)
    } else {
        None
    }
}

pub fn trial(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    let low = text.to_lowercase();
    if title == "accept" {
        return Some(BASE + 1.0);
    }
    if title == "double down" {
        return Some(-1000.0); // "Face lethal repercussions."
    }
    if title.starts_with("decide") {
        let mut score = BASE;
        for name in ["regret", "shame", "doubt"] {
            if low.contains(name) {
                score -= curse(name);
            }
        }
        if low.contains("2 random relics") {
            score += 2.0 * RELIC;
        }
        let m = num(r"obtain (\d+) gold", &low, -1);
        if m >= 0 {
            score += gold(m, ctx);
        }
        if low.contains("upgrade 2") {
            score += 2.0 * 3.5;
        }
        if low.contains("transform 2") {
            score += transforms(ctx, 2);
        }
        if low.contains("2 card rewards") {
            score += 2.0 * CARD_REWARD;
        }
        let m = num(r"heal (\d+)", &low, -1);
        if m >= 0 {
            score += heal(m, ctx);
        }
        return Some(score);
    }
    None
}

pub fn unrest_site(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // Poor Sleep is Unplayable and Retained: drawn once, it holds a card's place in every hand after.
    let (title, text) = _text(opt);
    if title == "rest anyways" {
        return Some(BASE + heal(ctx.max_hp, ctx) - curse("poor sleep"));
    }
    if title == "kill the trees" {
        return Some(BASE + RELIC - max_hp_loss(num(r"lose (\d+) max hp", &text, 0), ctx));
    }
    None
}

pub fn war_historian_repy(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, _) = _text(opt);
    if title == "unlock the chest" {
        return Some(BASE + 2.0 * RELIC + potions_gained(2, ctx, POTION));
    }
    if title == "unlock the cage" {
        return Some(BASE + RELIC + 4.0); // History Course: our last Attack played again every turn
    }
    None
}

pub fn waterfall_of_obvious_secrets(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // Dingy Dagger is a card (0 cost, 7 damage, upgraded any number of times), not a relic: Be Nice won 14 times in 14 as one.
    let (title, text) = _text(opt);
    if title == "be nice" {
        return Some(BASE + 0.8);
    }
    if title == "beat up" {
        return Some(BASE + RELIC - num(r"add (\d+) guilty", &text, 2) as f64 * curse("guilty"));
    }
    if title == "negotiate" {
        return Some(BASE + 2.5 + gold(-num(r"lose (\d+) gold", &text, 0), ctx)); // (a random Rare)
    }
    if title == "enter waterfall" {
        Some(BASE)
    } else {
        None
    }
}

pub fn waterlogged_scriptorium(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "bloody ink" {
        return Some(BASE + max_hp_gain(num(r"gain (\d+) max hp", &text, 6)));
    }
    if title == "prickly sponge" || title == "tentacle quill" {
        return Some(BASE + 1.0 * num(r"enchant (\d+) cards", &text, 1) as f64 + gold(-num(r"pay (\d+) gold", &text, 0), ctx)); // Steady: Retain
    }
    None
}

pub fn wellspring(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "bathe" {
        return Some(BASE + removals(ctx, 1) - num(r"add (\d+) guilty", &text, 1) as f64 * curse("guilty")); // (Guilty goes by itself after 5 fights)
    }
    if title == "bottle" {
        return Some(BASE + potions_gained(1, ctx, POTION));
    }
    None
}

pub fn whispering_hollow(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "exchange gold" {
        return Some(BASE + potions_gained(2, ctx, POTION) + gold(-num(r"lose (\d+) gold", &text, 0), ctx));
    }
    if title == "hug the tree" {
        return Some(BASE + transforms(ctx, 1) - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx));
    }
    None
}

pub fn wood_carvings(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // Slither ("randomize its cost from 0 to 3") is a loss on a card that costs 1: only a 2- or 3-cost card is worth it.
    use crate::cards::parse;
    let (title, _) = _text(opt);
    if title == "snake" {
        let dear = ctx.deck.iter().any(|c| parse(c).cost >= 2);
        return Some(BASE + (if dear { 1.5 } else { -0.5 }));
    }
    if title == "bird" || title == "torus" {
        return Some(BASE + (if counts(ctx)["basics"] != 0 || ctx.deck.is_empty() { 2.5 } else { 0.0 })); // (a Strike into Peck, a Defend into Toric Toughness)
    }
    None
}

pub fn wuppy_in_trouble(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // Wuppy soaks every attack's damage until its HP is gone: 60 HP of it for an Injury beats 30 for 2 HP.
    let (title, text) = _text(opt);
    let soak = |n: i64| 0.6 * 12.0 * n as f64 / ctx.max_hp.max(1) as f64;
    if title == "be cautious" {
        return Some(BASE + soak(num(r"with (\d+) hp", &text, 30)) - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx));
    }
    if title == "help immediately" {
        return Some(BASE + soak(num(r"with (\d+) hp", &text, 60)) - curse("injury"));
    }
    if title == "ignore" {
        return Some(BASE + potions_gained(1, ctx, POTION));
    }
    None
}

pub fn zen_weaver(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    let price = gold(-num(r"pay (\d+) gold", &text, 0), ctx);
    if title == "emotional awareness" {
        return Some(BASE + removals(ctx, 1) + price);
    }
    if title == "arachnid acupuncture" {
        return Some(BASE + removals(ctx, 2) + price);
    }
    if title == "breathing techniques" {
        return Some(BASE + 0.3 + price); // (Enlightenment: costs of the hand to 1 for a turn)
    }
    None
}

#[rustfmt::skip]
pub static RULES: LazyLock<IndexMap<&'static str, Rule>> = LazyLock::new(|| {
    let rows: Vec<(&'static str, Rule)> = vec![
        ("ABYSSAL_BATHS", abyssal_baths), ("AMALGAMATOR", amalgamator), ("AROMA_OF_CHAOS", aroma_of_chaos), ("BATTLEWORN_DUMMY", battleworn_dummy), ("BRAIN_LEECH", brain_leech),
        ("COLORFUL_PHILOSOPHERS", colorful_philosophers), ("COLOSSAL_FLOWER", colossal_flower), ("CRYSTAL_SPHERE", crystal_sphere), ("DENSE_VEGETATION", dense_vegetation),
        ("DOLL_ROOM", doll_room), ("DOORS_OF_LIGHT_AND_DARK", doors_of_light_and_dark), ("DROWNING_BEACON", drowning_beacon), ("ENDLESS_CONVEYOR", endless_conveyor),
        ("FIELD_OF_MAN_SIZED_HOLES", field_of_man_sized_holes), ("GRAVE_OF_THE_FORGOTTEN", grave_of_the_forgotten), ("HUNGRY_FOR_MUSHROOMS", hungry_for_mushrooms),
        ("JUNGLE_MAZE_ADVENTURE", jungle_maze_adventure), ("LOST_WISP", lost_wisp), ("LUMINOUS_CHOIR", luminous_choir), ("MORPHIC_GROVE", morphic_grove),
        ("MUSEUM_OF_TORTURE", museum_of_torture), ("ROUND_TEA_PARTY", round_tea_party), ("ROOM_FULL_OF_CHEESE", room_full_of_cheese), ("SPIRALING_WHIRLPOOL", spiraling_whirlpool),
        ("SPIRIT_GRAFTER", spirit_grafter), ("STONE_OF_ALL_TIME", stone_of_all_time), ("SUNKEN_STATUE", sunken_statue), ("SUNKEN_TREASURY", sunken_treasury), ("SYMBIOTE", symbiote),
        ("TABLET_OF_TRUTH", tablet_of_truth), ("THE_LANTERN_KEY", the_lantern_key), ("THE_LEGENDS_WERE_TRUE", the_legends_were_true), ("THIS_OR_THAT", this_or_that),
        ("TINKER_TIME", tinker_time), ("TRIAL", trial), ("UNREST_SITE", unrest_site), ("WAR_HISTORIAN_REPY", war_historian_repy), ("WATERFALL_OF_OBVIOUS_SECRETS", waterfall_of_obvious_secrets),
        ("WATERLOGGED_SCRIPTORIUM", waterlogged_scriptorium), ("WELLSPRING", wellspring), ("WHISPERING_HOLLOW", whispering_hollow), ("WOOD_CARVINGS", wood_carvings),
        ("WUPPY_IN_TROUBLE", wuppy_in_trouble), ("ZEN_WEAVER", zen_weaver),
    ];
    rows.into_iter().collect()
});
/// Every event in the game (localization/eng/events.json, v0.113.1), and the ones the generic scorer is left to: checked one by one.
#[rustfmt::skip]
pub static GENERIC_CHECKED: LazyLock<IndexMap<&'static str, &'static str>> = LazyLock::new(|| IndexMap::from([
    ("BUGSLAYER", "two cards by their ratings"), ("BYRDONIS_NEST", "7 Max HP against an egg to hatch"), ("INFESTED_AUTOMATON", "two random cards"),
    ("POTION_COURIER", "one Uncommon potion against three Foul ones"), ("PUNCH_OFF", "the fight priced by EVENT_FIGHT_HP"), ("RANWID_THE_ELDER", "relic trades by relicworth"),
    ("REFLECTIONS", "upgrades against a doubled deck and Bad Luck"), ("RELIC_TRADER", "relic trades by relicworth"), ("SAPPHIRE_SEED", "heal and upgrade against Sown"),
    ("SELF_HELP_BOOK", "an enchantment a card type"), ("SHRINE_OF_THE_WARLORD", "Mighty Wings and Clumsy against two cards"), ("SLIPPERY_BRIDGE", "the named card against HP"),
    ("TEA_MASTER", "three small teas"), ("THE_FUTURE_OF_POTIONS", "a potion for an upgraded card"), ("TRASH_HEAP", "HP for a relic against gold and a card"),
    ("WELCOME_TO_WONGOS", "relics for gold, the Mystery Box by _lives_through"), ("THERMAE_NONUPEIPE", "relics by name"), ("MOCK_EVENT_MODEL", "not in play"),
]));
/// (A set in the original: only asked whether an event is in it.)
pub static COVERED: LazyLock<BTreeSet<&'static str>> = LazyLock::new(|| RULES.keys().chain(GENERIC_CHECKED.keys()).copied().collect());

pub fn value(event_id: &str, opt: &Value, ctx: &Ctx) -> Option<f64> {
    let rule = RULES.get(event_id)?;
    rule(opt, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::events::tests::{ctx_of, resolve, world};

    /// Every rule over the options of its own event in a grid of contexts (hand-written, and the decks, belts and relics of the
    /// model runs), and over every other option text on record in a few: as the original, under both games.
    #[test]
    fn golden() {
        let g: Value = serde_json::from_str(include_str!("../../../tests/golden/events/sts2.json")).unwrap();
        let pool = g["pool"].list();
        let ctxs: Vec<Ctx> = g["ctxs"].list().iter().map(|c| ctx_of(&resolve(c, pool))).collect();
        let opts = g["opts"].list();
        let small: Vec<usize> = g["small"].list().iter().map(|n| n.as_u64().unwrap() as usize).collect();
        let (mut checked, mut numbers) = (0, 0);
        for game in ["sts2", "sts1"] {
            let knobs: serde_json::Map<String, Value> = crate::learn::defaults(Some(game)).into_iter().map(|(k, x)| (k, json!(x))).collect();
            let _world = world(&json!(game), &Value::Object(knobs), &Value::Null);
            for row in g["own"][game].list() {
                let (event, opt) = (row[0].s(), &opts[row[1].as_u64().unwrap() as usize]);
                let over: Vec<usize> = if game == "sts2" { (0..ctxs.len()).collect() } else { small.clone() };
                assert_eq!(over.len(), row[2].list().len());
                for (c, want) in over.iter().zip(row[2].list()) {
                    assert_eq!(value(event, opt, &ctxs[*c]), want.as_f64(), "{game} {event} {opt} in {}", g["ctxs"][*c]);
                    checked += 1;
                    numbers += want.is_number() as usize;
                }
            }
            let cross: IndexMap<(&str, usize), &Value> = g["cross"][game].list().iter().map(|row| ((row[0].s(), row[1].as_u64().unwrap() as usize), &row[2])).collect();
            for event in RULES.keys().copied().chain(["NOT_AN_EVENT", "BUGSLAYER", ""]) {
                for (n, opt) in opts.iter().enumerate() {
                    let want = cross.get(&(event, n));
                    for (at, c) in small.iter().enumerate() {
                        assert_eq!(value(event, opt, &ctxs[*c]), want.and_then(|w| w[at].as_f64()), "{game} {event} {opt} in {}", g["ctxs"][*c]);
                        checked += 1;
                    }
                }
            }
        }
        for (n, ctx) in ctxs.iter().enumerate() {
            assert_eq!(three_turns(ctx), g["three_turns"][n].as_f64().unwrap(), "three_turns {}", g["ctxs"][n]);
            assert_eq!(_hallway_fight(ctx), g["hallway"][n].as_f64().unwrap(), "_hallway_fight {}", g["ctxs"][n]);
        }
        for row in g["relic"].list() {
            for (at, c) in small.iter().enumerate() {
                assert_eq!(relic(row[0].s(), &ctxs[*c]), row[1][at].as_f64().unwrap(), "relic {}", row[0]);
            }
        }
        for row in g["text"].list() {
            let (title, text) = _text(&row[0]);
            assert_eq!(json!([title, text]), row[1], "_text {}", row[0]);
        }
        assert!(checked > 1_500_000 && numbers > 45000, "{checked} golden cases, {numbers} numbers");
    }

    #[test]
    fn tables_as_recorded() {
        let g: Value = serde_json::from_str(include_str!("../../../tests/golden/events/sts2.json")).unwrap();
        assert_eq!(json!(RULES.keys().collect::<Vec<_>>()), g["rules"]);
        let checked: serde_json::Map<String, Value> = GENERIC_CHECKED.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
        assert_eq!(base::json_dumps(&Value::Object(checked)), base::json_dumps(&g["generic_checked"]));
        assert_eq!(json!(COVERED.iter().collect::<Vec<_>>()), g["covered"]);
        let colour: serde_json::Map<String, Value> = _COLOUR.iter().map(|(k, v)| (k.to_string(), json!([v.0, v.1]))).collect();
        assert_eq!(base::json_dumps(&Value::Object(colour)), base::json_dumps(&g["colour"]));
        let rider: serde_json::Map<String, Value> = _RIDER.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
        assert_eq!(base::json_dumps(&Value::Object(rider)), base::json_dumps(&g["rider"]));
        assert_eq!(json!(_BELT.keys().collect::<Vec<_>>()), g["belt"]);
        assert_eq!((BASE, MEET_WAR_HISTORIAN), (g["base"].as_f64().unwrap(), g["meet"].as_f64().unwrap()));
    }
}
