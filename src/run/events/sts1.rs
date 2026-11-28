//! The first game's events, one rule each (2026-11-26: the game's texts from desktop-1.0.jar localization/eng/events.json, what
//! the bot met and chose in its logs). A rule returns an option's score on the generic scorer's scale (a relic 6, an upgrade 4),
//! or None to leave it to that scorer; `COVERED` names every event and what was checked of those left to it.
//!
//! The shape (the same for sts2.rs): a rule is a plain function `fn(opt: &Value, ctx: &Ctx) -> Option<f64>` ([`Rule`]), named
//! as in the original; `RULES` maps an event's id to its rule (an `IndexMap` in the original's row order); `value(event_id, opt, ctx)`
//! looks the rule up and calls it. (the original's `events.rule_value` catches any exception of a rule and gives None: these rules
//! raise nothing on options and decks made of JSON objects; only a number too long for an i64 in a text would panic, in
//! `value::num`.)

use std::collections::BTreeSet;
use std::sync::LazyLock;

use indexmap::IndexMap;
use serde_json::{json, Value};

use super::value::{
    card_worth, curse, gold, heal, hp_cost, is_curse, max_hp_gain, max_hp_loss, name_of, num, potions_gained, random_upgrades, removals, transforms, Ctx, CARD_REWARD,
    ETERNAL, POTION, RELIC, UPGRADE,
};
use crate::base::{self, fmax, fmin, strip, JsonExt};
use crate::run::relicworth::{id_of as relic_id, worth as relic_worth};

pub const BASE: f64 = 0.5;
pub const RUN: f64 = 25.0; // what the rest of the run is worth, in these points: a fight that may end it is priced by its chance of doing so

/// An event's rule: the option's score, or None to leave it to the generic scorer.
pub type Rule = fn(&Value, &Ctx) -> Option<f64>;

pub fn _text(opt: &Value) -> (String, String) {
    (strip(&opt["title"].text()).to_lowercase(), opt["description"].text())
}

/// A fight an event starts: the HP it is expected to cost (a share of Max HP).
pub fn _fight(share: f64, ctx: &Ctx) -> f64 {
    hp_cost(base::round(share * ctx.max_hp as f64), ctx)
}

/// What losing the card an option names costs (a Strike or a curse gone is a gain).
pub fn _lost_card(text: &str, ctx: &Ctx) -> f64 {
    let Some(m) = crate::re!(r"(?i)lose ([a-z' +.-]+?)(?:\.|\n?\z)").captures(text) else { return 0.0 };
    let named = m.get(1).map_or("", |g| g.as_str());
    let wanted = name_of(named);
    let card = match ctx.deck.iter().find(|c| name_of(*c) == wanted) {
        Some(c) => c.clone(),
        None => json!({"name": strip(named)}),
    };
    if is_curse(&card) {
        return -(curse(&name_of(&card)) + 1.0);
    }
    use crate::cards::is_basic;
    if is_basic(&card) || is_basic(&json!({"name": card["name"].text().trim_end_matches('+')})) {
        return -3.5;
    }
    0.8 * fmax(0.0, card_worth(&card) * 2.0 + 3.0 - 2.0)
}

pub fn back_to_basics(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // Elegance won 33 times in 33, at 5.5 against 4.5: upgrading every Strike and Defend is eight upgrades in an early deck.
    let (title, _) = _text(opt);
    if title == "elegance" {
        return Some(BASE + removals(ctx, 1));
    }
    if title == "simplicity" {
        let basics = ctx
            .deck
            .iter()
            .filter(|c| {
                let name = name_of(*c);
                (name.contains("strike") || name.contains("defend")) && !c["name"].text().ends_with('+')
            })
            .count();
        return Some(if !ctx.deck.is_empty() { BASE + fmin(10.0, 1.3 * basics as f64) } else { BASE + 6.0 });
    }
    None
}

pub fn beggar(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "offer gold" {
        return Some(BASE + removals(ctx, 1) + gold(-num(r"(\d+) gold", &text, 75), ctx));
    }
    None
}

pub fn big_fish(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "banana" {
        return Some(BASE + heal(num(r"heal (\d+)", &text, 0), ctx));
    }
    if title == "donut" {
        return Some(BASE + max_hp_gain(num(r"max hp \+(\d+)", &text, 5)));
    }
    if title == "box" {
        return Some(BASE + RELIC - curse("regret"));
    }
    None
}

pub fn colosseum(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // VICTORY: two elites at once (Taskmaster and Gremlin Nob) for two relics, gold and a card; for most decks the end of the run.
    let (title, _) = _text(opt);
    if title == "victory" {
        let death = if ctx.hp as f64 >= 0.9 * ctx.max_hp as f64 { 0.4 } else { 0.7 }; // (two elites at once: the bot loses a sixth of single act 2 elites)
        return Some(BASE + 2.0 * RELIC + gold(100, ctx) + CARD_REWARD - _fight(0.6, ctx) - death * RUN);
    }
    if title == "cowardice" {
        return Some(BASE);
    }
    None
}

/// Read on (1, 2, 3 HP) and take the book (10 HP: Necronomicon, Enchiridion or Nilry's Codex), or stop for 3. What has been
/// paid is gone: each page is priced by what is still to come.
pub fn cursed_tome(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    let book = RELIC + 1.0;
    if title == "read" {
        return Some(BASE + book - hp_cost(1 + 2 + 3 + 10, ctx));
    }
    if title == "continue" {
        let step = num(r"lose (\d+) hp", &text, 1);
        let rest = match step {
            1 => 2 + 3 + 10,
            2 => 3 + 10,
            _ => 10,
        };
        return Some(BASE + fmax(book - hp_cost(step + rest, ctx), -hp_cost(step + 3, ctx)));
    }
    if title == "take" {
        return Some(BASE + book - hp_cost(num(r"lose (\d+) hp", &text, 10), ctx));
    }
    if title == "stop" {
        return Some(BASE - hp_cost(num(r"lose (\d+) hp", &text, 3), ctx));
    }
    None
}

pub fn dead_adventurer(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // Three searches hold 30 Gold, a relic and nothing; each may bring the elite back (its rewards come with its fight).
    let (title, text) = _text(opt);
    if title == "search" || title == "continue" {
        let p = num(r"(\d+)%", &text, 25) as f64 / 100.0;
        let loot = (RELIC + gold(30, ctx)) / 3.0;
        return Some(BASE + (1.0 - p) * loot + p * (RELIC + CARD_REWARD - _fight(0.3, ctx)));
    }
    None
}

pub fn designer(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    let low = text.to_lowercase();
    let price = gold(-num(r"lose (\d+) gold", &text, 0), ctx);
    if title == "adjustments" {
        return Some(BASE + price + (if low.contains("upgrade a card") { UPGRADE } else { random_upgrades(2, ctx) }));
    }
    if title == "clean up" {
        return Some(BASE + price + (if low.contains("remove a card") { removals(ctx, 1) } else { transforms(ctx, 2) }));
    }
    if title == "full service" {
        return Some(BASE + price + removals(ctx, 1) + random_upgrades(1, ctx));
    }
    if title == "punch" {
        return Some(BASE - hp_cost(num(r"lose (\d+) hp", &text, 3), ctx));
    }
    None
}

pub fn drug_dealer(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, _) = _text(opt);
    if title == "ingest mutagens" {
        return Some(BASE + RELIC - 2.0); // Mutagenic Strength: 3 Strength for the first turn of each fight
    }
    if title == "become test subject" {
        return Some(BASE + transforms(ctx, 2));
    }
    if title == "test j.a.x." {
        return Some(BASE + 0.5);
    }
    None
}

pub fn duplicator(opt: &Value, _ctx: &Ctx) -> Option<f64> {
    let (title, _) = _text(opt);
    if title == "pray" {
        Some(BASE + 2.0) // (the best card, twice)
    } else {
        None
    }
}

pub fn facetrader(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "touch" {
        return Some(BASE + gold(num(r"gain (\d+) gold", &text, 0), ctx) - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx));
    }
    if title == "trade" {
        return Some(BASE + 0.5); // (half a good face: Face of Cleric, Ssserpent Head; half a bad one: Gremlin Visage, N'loth's Hungry Face)
    }
    None
}

pub fn falling(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // Each option loses the card it names (one of each type at random): all three were 0.5 and the first went, whatever it was.
    let (title, text) = _text(opt);
    if title == "land" || title == "channel" || title == "strike" {
        return Some(BASE - _lost_card(&text, ctx));
    }
    None
}

pub fn forgotten_altar(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title.starts_with("offer") {
        return Some(BASE + RELIC - 3.0 + 1.0); // Bloody Idol for the Golden Idol
    }
    if title == "sacrifice" {
        return Some(BASE + max_hp_gain(num(r"gain (\d+) max hp", &text, 5)) - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx));
    }
    if title == "desecrate" {
        return Some(BASE - curse("decay"));
    }
    None
}

pub fn fountain_of_cleansing(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, _) = _text(opt);
    if title == "drink" {
        return Some(BASE + base::fsum(ctx.deck.iter().filter(|c| is_curse(c) && !ETERNAL.contains(name_of(*c).as_str())).map(|c| curse(&name_of(c)) + 1.0)));
    }
    None
}

pub fn golden_shrine(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "pray" || title == "desecrate" {
        return Some(BASE + gold(num(r"gain (\d+) gold", &text, 0), ctx) - (if text.to_lowercase().contains("regret") { curse("regret") } else { 0.0 }));
    }
    None
}

pub fn golden_wing(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "pray" {
        return Some(BASE + removals(ctx, 1) - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx));
    }
    if title == "destroy" {
        let (lo, hi) = (num(r"gain (\d+)", &text, 50), num(r"-\s*(\d+)", &text, 80));
        return Some(BASE + gold(base::floordiv(lo + hi, 2), ctx));
    }
    None
}

pub fn lab(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, _) = _text(opt);
    if title == "search" {
        Some(BASE + potions_gained(3, ctx, POTION))
    } else {
        None
    }
}

pub fn liars_game(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "agree" {
        return Some(BASE + gold(num(r"gain (\d+) gold", &text, 0), ctx) - curse("doubt"));
    }
    if title == "disagree" {
        Some(BASE)
    } else {
        None
    }
}

pub fn living_wall(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, _) = _text(opt);
    if title == "forget" {
        return Some(BASE + removals(ctx, 1));
    }
    if title == "change" {
        return Some(BASE + transforms(ctx, 1));
    }
    if title == "grow" {
        return Some(BASE + UPGRADE);
    }
    None
}

pub fn masked_bandits(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // "Lose ALL of your Gold" was read as the end of the run (-999.5): it is the gold we hold, and their fight can cost a third of our HP.
    let (title, _) = _text(opt);
    if title == "pay" {
        return Some(BASE + gold(-ctx.gold, ctx));
    }
    if title == "fight!" {
        return Some(BASE + RELIC - 4.0 + CARD_REWARD + gold(30, ctx) - _fight(0.3, ctx)); // (Red Mask, gold, a card)
    }
    None
}

pub fn mushrooms(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "stomp" {
        return Some(BASE + RELIC - 3.0 + CARD_REWARD - _fight(0.15, ctx)); // (three Fungi Beasts, Odd Mushroom)
    }
    if title == "eat" {
        return Some(BASE + heal(num(r"heal (\d+)", &text, 0), ctx) - curse("parasite"));
    }
    None
}

pub fn mysterious_sphere(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, _) = _text(opt);
    if title == "open sphere" {
        return Some(BASE + RELIC + 2.0 + CARD_REWARD - _fight(0.4, ctx)); // (two Orb Walkers for a Rare relic: one alone costs a fifth of Max HP)
    }
    None
}

/// Neow's blessings, by what each does (both the small four after a short run and the full set): a random boss relic
/// for the starter one is the strongest start there is for most decks; three fights of 1-HP enemies cost nothing.
pub fn neow_event(opt: &Value, _ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    let low = format!("{title} {text}").to_lowercase();
    if low.contains("boss relic") && low.contains("starting relic") {
        return Some(BASE + 3.5); // (most boss relics are an energy a turn; Runic Dome would hide the intents the planner reads)
    }
    if low.contains("next three combats have 1 hp") {
        return Some(BASE + 2.5);
    }
    None
}

pub fn nest(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "stay in line" {
        return Some(BASE + 1.5 - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx)); // Ritual Dagger: 15 damage, more for each kill
    }
    if title == "smash and grab" {
        return Some(BASE + gold(num(r"obtain (\d+) gold", &text, 0), ctx));
    }
    None
}

pub fn note_for_yourself(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // The card stored for the next run leaves this deck: the worst one (strategy.card_select takes "store" worst-first).
    let (title, text) = _text(opt);
    if title == "take and give" {
        let m = crate::re!(r"(?i)receive ([a-z' +.-]+?) and store").captures(&text);
        let got = match m {
            Some(m) => card_worth(m.get(1).map_or("", |g| g.as_str())) + 1.0,
            None => 0.5,
        };
        return Some(BASE + got + removals(ctx, 1) * 0.8);
    }
    if title == "ignore" {
        Some(BASE)
    } else {
        None
    }
}

pub fn n_loth(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // N'loth's Gift (three times the Rare cards) for one of our relics: given 7 times in 7 at 0.5 each, Pure Water and Cauldron among them.
    let (title, _) = _text(opt);
    if title.starts_with("offer") {
        let m = crate::re!(r"^offer:?\s*(.+)").captures(&title);
        let held = match m {
            Some(m) => relic_id(m.get(1).map_or("", |g| g.as_str())),
            None => String::new(),
        };
        return Some(BASE + 2.0 - (RELIC + relic_worth(&held, &ctx.deck, Some(ctx.act), ctx.hp, ctx.max_hp)));
    }
    None
}

pub fn purifier(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, _) = _text(opt);
    if title == "pray" {
        Some(BASE + removals(ctx, 1))
    } else {
        None
    }
}

pub fn scrap_ooze(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // Each reach costs a little more HP for a better chance at a relic: priced on its own, not as one more bite of a bleeding event.
    let (title, text) = _text(opt);
    if title == "reach inside" || title == "deeper" {
        let p = num(r"(\d+)%", &text, 25) as f64 / 100.0;
        return Some(BASE + p * RELIC - hp_cost(num(r"lose (\d+) hp", &text, 3), ctx));
    }
    None
}

pub fn sensory_stone(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "recall" {
        return Some(BASE + 0.9 * num(r"add (\d+) colorless", &text, 1) as f64 - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx));
    }
    None
}

pub fn shining_light(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "enter" {
        return Some(BASE + random_upgrades(2, ctx) - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx));
    }
    None
}

pub fn the_cleric(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    let price = gold(-num(r"(\d+) gold", &text, 0), ctx);
    if title == "heal" {
        return Some(BASE + heal(num(r"heal (\d+)", &text, 0), ctx) + price);
    }
    if title == "purify" {
        return Some(BASE + removals(ctx, 1) + price);
    }
    None
}

pub fn the_joust(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "murderer" || title == "owner" {
        let (bet, win) = (num(r"bet (\d+)", &text, 50), num(r"win (\d+)", &text, 100));
        let p = num(r"(\d+)%", &text, 70) as f64 / 100.0;
        return Some(BASE + p * gold(win - bet, ctx) + (1.0 - p) * gold(-bet, ctx));
    }
    None
}

pub fn the_library(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // A card of our choosing from twenty: the best a reward can be (it lost to a heal of a few HP at 1.5).
    let (title, text) = _text(opt);
    if title == "read" {
        return Some(BASE + 3.0);
    }
    if title == "sleep" {
        return Some(BASE + heal(num(r"heal (\d+)", &text, 0), ctx));
    }
    None
}

pub fn the_mausoleum(opt: &Value, _ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "open coffin" {
        return Some(BASE + RELIC - num(r"(\d+)%", &text, 50) as f64 / 100.0 * curse("writhe"));
    }
    None
}

pub fn the_moai_head(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "jump inside" {
        return Some(BASE + heal(ctx.max_hp, ctx) - max_hp_loss(num(r"lose (\d+) max hp", &text, 0), ctx));
    }
    if title.starts_with("offer") {
        return Some(BASE + gold(num(r"gain (\d+) gold", &text, 333), ctx) - 2.0); // (the Golden Idol's quarter more of the gold still to come goes with it)
    }
    None
}

pub fn the_woman_in_blue(opt: &Value, ctx: &Ctx) -> Option<f64> {
    // Three potions for 40 Gold beat one for 20 with room on the belt: all three were 4.0 and one was bought every time.
    let (title, text) = _text(opt);
    if let Some(m) = crate::re!(r"^buy (\d+) potions?").captures(&title) {
        let digits = m.get(1).map_or("", |g| g.as_str());
        let n: i64 = digits.parse().unwrap_or_else(|_| panic!("invalid literal for int() with base 10: {}", base::str_repr(digits)));
        return Some(BASE + potions_gained(n, ctx, POTION) + gold(-num(r"(\d+) gold", &text, 0), ctx));
    }
    if title == "leave" {
        return Some(BASE - hp_cost(num(r"take (\d+) damage", &text, 0), ctx));
    }
    None
}

pub fn thinking_with_puddles(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "gooped" {
        return Some(BASE - 3.5); // (a Rare card of ours)
    }
    if title == "land face first" {
        return Some(BASE - hp_cost(num(r"take (\d+) damage", &text, 0), ctx) - curse("injury"));
    }
    None
}

pub fn tomb_of_lord_red_mask(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title.starts_with("offer") {
        return Some(BASE + RELIC + 0.5 + gold(-ctx.gold, ctx)); // Red Mask for all our gold (-993.5 before: "Lose all Gold" was ruin)
    }
    if title == "don the red mask" {
        return Some(BASE + gold(num(r"gain (\d+) gold", &text, 222), ctx));
    }
    None
}

pub fn transmorgrifier(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, _) = _text(opt);
    if title == "pray" {
        Some(BASE + transforms(ctx, 1))
    } else {
        None
    }
}

pub fn upgrade_shrine(opt: &Value, _ctx: &Ctx) -> Option<f64> {
    let (title, _) = _text(opt);
    if title == "pray" {
        Some(BASE + UPGRADE)
    } else {
        None
    }
}

pub fn we_meet_again(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "give card" {
        return Some(BASE + RELIC - _lost_card(&text, ctx));
    }
    if title == "give gold" {
        return Some(BASE + RELIC + gold(-num(r"lose (\d+) gold", &text, 0), ctx));
    }
    if title == "give potion" {
        return Some(BASE + RELIC - POTION - 0.5);
    }
    if title == "attack" {
        Some(BASE)
    } else {
        None
    }
}

pub fn winding_halls(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "embrace madness" {
        return Some(BASE + 2.0 * 0.8 - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx)); // (Madness: a card of the hand free)
    }
    if title == "retrace your steps" {
        return Some(BASE - max_hp_loss(num(r"lose (\d+) max hp", &text, 0), ctx));
    }
    if title == "focus" {
        return Some(BASE + heal(num(r"heal (\d+)", &text, 0), ctx) - curse("writhe"));
    }
    None
}

pub fn world_of_goop(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "gather gold" {
        return Some(BASE + gold(num(r"gain (\d+) gold", &text, 0), ctx) - hp_cost(num(r"lose (\d+) hp", &text, 0), ctx));
    }
    if title == "leave it" {
        return Some(BASE + gold(-num(r"lose (\d+) gold", &text, 0), ctx));
    }
    None
}

pub fn addict(opt: &Value, ctx: &Ctx) -> Option<f64> {
    let (title, text) = _text(opt);
    if title == "offer gold" {
        return Some(BASE + RELIC + gold(-num(r"(\d+) gold", &text, 85), ctx));
    }
    if title == "rob" {
        return Some(BASE + RELIC - curse("shame"));
    }
    None
}

#[rustfmt::skip]
pub static RULES: LazyLock<IndexMap<&'static str, Rule>> = LazyLock::new(|| {
    let rows: Vec<(&'static str, Rule)> = vec![
        ("ADDICT", addict), ("BACK_TO_BASICS", back_to_basics), ("BEGGAR", beggar), ("BIG_FISH", big_fish), ("COLOSSEUM", colosseum), ("CURSED_TOME", cursed_tome),
        ("DEAD_ADVENTURER", dead_adventurer), ("DESIGNER", designer), ("DRUG_DEALER", drug_dealer), ("DUPLICATOR", duplicator), ("FACETRADER", facetrader), ("FALLING", falling),
        ("FORGOTTEN_ALTAR", forgotten_altar), ("FOUNTAIN_OF_CLEANSING", fountain_of_cleansing), ("GOLDEN_SHRINE", golden_shrine), ("GOLDEN_WING", golden_wing), ("LAB", lab),
        ("LIARS_GAME", liars_game), ("LIVING_WALL", living_wall), ("MASKED_BANDITS", masked_bandits), ("MUSHROOMS", mushrooms), ("MYSTERIOUS_SPHERE", mysterious_sphere),
        ("NEOW_EVENT", neow_event), ("NEST", nest), ("NOTEFORYOURSELF", note_for_yourself), ("N_LOTH", n_loth), ("PURIFIER", purifier), ("SCRAP_OOZE", scrap_ooze),
        ("SENSORYSTONE", sensory_stone), ("SHINING_LIGHT", shining_light), ("THE_CLERIC", the_cleric), ("THE_JOUST", the_joust), ("THE_LIBRARY", the_library),
        ("THE_MAUSOLEUM", the_mausoleum), ("THE_MOAI_HEAD", the_moai_head), ("THE_WOMAN_IN_BLUE", the_woman_in_blue), ("THINKING_WITH_PUDDLES", thinking_with_puddles),
        ("TOMB_OF_LORD_RED_MASK", tomb_of_lord_red_mask), ("TRANSMORGRIFIER", transmorgrifier), ("UPGRADE_SHRINE", upgrade_shrine), ("WEMEETAGAIN", we_meet_again),
        ("WINDING_HALLS", winding_halls), ("WORLD_OF_GOOP", world_of_goop),
    ];
    rows.into_iter().collect()
});
#[rustfmt::skip]
pub static GENERIC_CHECKED: LazyLock<IndexMap<&'static str, &'static str>> = LazyLock::new(|| IndexMap::from([
    ("ACCURSED_BLACKSMITH", "an upgrade against Warped Tongs and Pain (edit 217)"), ("BONFIRE_ELEMENTALS", "the offer is the card select's, worst first"),
    ("GHOSTS", "half our Max HP for five Apparitions: refused (the planner does not price our Intangible)"), ("GOLDEN_IDOL", "the idol, then the least of three traps"),
    ("KNOWING_SKULL", "PAID_EXIT (edit 228)"), ("MATCH_AND_KEEP", "a memory game the option scores cannot play: first option"), ("MINDBLOOM", "edit 239"),
    ("SECRETPORTAL", "TO_BOSS (edit 237)"), ("SPIRE_HEART", "act 4's door"), ("VAMPIRES", "a third of our Max HP for Bites: refused"), ("WHEEL_OF_CHANGE", "one option"),
    ("PROCEED_SCREEN", "not an event"),
]));
/// (A set in the original: only asked whether an event is in it.)
pub static COVERED: LazyLock<BTreeSet<&'static str>> = LazyLock::new(|| RULES.keys().chain(GENERIC_CHECKED.keys()).copied().collect());

/// Match and Keep (user, 2026-11-28: find the pairs, not the two cards top left). The options are the face-down cards,
/// named once seen, else `cardN`; `first`: the card face up while we pick the second. A pair taken goes into the deck, so a
/// curse's pair is never completed. Returns the options in order of preference.
pub fn match_pick(titles: &[&str], first: Option<&str>) -> Vec<usize> {
    let key = |t: &str| t.chars().filter(char::is_ascii_alphabetic).collect::<String>().to_uppercase();
    let unseen = |t: &str| t.to_lowercase().strip_prefix("card").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    let curse = |t: &str| super::_JUNK_NAMES.contains(&key(t).as_str()); // (the game deals ordinary curses only: all listed)
    let second = first.is_some() || titles.len() % 2 == 1; // (a matched pair leaves two fewer: odd means one is face up)
    let rank = |i: usize| -> u8 {
        let t = titles[i];
        let pairs = first.map_or(false, |f| key(f) == key(t));
        if second && first.is_some_and(curse) {
            // anything but its partner: a seen card of another name is sure not to be it
            return if pairs { 3 } else if unseen(t) { 1 } else { 0 };
        }
        if pairs {
            return 0;
        }
        if !second && !unseen(t) && !curse(t) && titles.iter().enumerate().any(|(j, u)| j != i && key(u) == key(t) && !unseen(u)) {
            return 0; // half of a pair we know
        }
        if unseen(t) {
            1 // turn up a new one
        } else if curse(t) {
            3
        } else {
            2
        }
    };
    let mut order: Vec<usize> = (0..titles.len()).collect();
    order.sort_by_key(|&i| rank(i));
    order
}

pub fn value(event_id: &str, opt: &Value, ctx: &Ctx) -> Option<f64> {
    let rule = RULES.get(event_id)?;
    rule(opt, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games;

    #[test]
    fn match_and_keep_takes_known_pairs_and_never_a_curse() {
        // first pick: half of a pair known, else a new card
        assert_eq!(match_pick(&["card0", "Zap", "card2", "Zap"], None)[0], 1);
        assert_eq!(match_pick(&["Zap", "card1", "Bash", "card3"], None)[0], 1);
        assert_eq!(match_pick(&["Writhe", "card1", "Writhe", "card3"], None)[0], 1); // a curse's pair is left alone
        // second pick (an odd number face down): the partner of the card face up, else a new one
        assert_eq!(match_pick(&["card0", "Bash", "Zap"], Some("Zap"))[0], 2);
        assert_eq!(match_pick(&["Bash", "card1", "Zap"], Some("Defragment"))[0], 1);
        assert_eq!(match_pick(&["card0", "Writhe", "Bash"], Some("Writhe"))[0], 2); // not the curse's partner, for sure
    }

    fn ctx_of(v: &Value, cards: &[Value]) -> Ctx {
        Ctx {
            hp: v["hp"].as_i64().unwrap(),
            max_hp: v["max_hp"].as_i64().unwrap(),
            gold: v["gold"].as_i64().unwrap(),
            act: v["act"].as_i64().unwrap(),
            floor: v["floor"].as_i64().unwrap(),
            deck: v["deck"].list().iter().map(|n| cards[n.as_u64().unwrap() as usize].clone()).collect(),
            relics: v["relics"].list().to_vec(),
            potions: v["potions"].list().to_vec(),
            slots: v["slots"].as_i64().unwrap(),
            character: v["character"].s().into(),
            game: v["game"].s().into(),
            bled: v["bled"].as_i64().unwrap(),
            keys: v["keys"].as_array().cloned(),
        }
    }

    /// Every rule over the options of its own event, of the tests and of the model runs, in a grid of contexts: as the original.
    #[test]
    fn golden() {
        let g: Value = serde_json::from_str(include_str!("../../../tests/golden/events_sts1.json")).unwrap();
        let cards = g["cards"].list();
        let ctxs: Vec<Ctx> = g["ctxs"].list().iter().map(|c| ctx_of(c, cards)).collect();
        let opts = g["opts"].list();
        let sets: Vec<Vec<usize>> = g["sets"].list().iter().map(|s| s.list().iter().map(|n| n.as_u64().unwrap() as usize).collect()).collect();
        let game_of = |name: &Value| if name.s() == "sts1" { "sts1" } else { "sts2" };
        let (mut checked, mut numbers) = (0, 0);
        for case in g["cases"].list() {
            let _game = games::playing(game_of(&case[0]));
            let (event, opt) = (case[1].s(), &opts[case[2].as_u64().unwrap() as usize]);
            for (c, want) in sets[case[3].as_u64().unwrap() as usize].iter().zip(case[4].list()) {
                assert_eq!(value(event, opt, &ctxs[*c]), want.as_f64(), "{event} {opt} in {}", g["ctxs"][*c]);
                checked += 1;
                numbers += want.is_number() as usize;
            }
        }
        for row in g["nones"].list() {
            let _game = games::playing(game_of(&row[0]));
            for o in row[3].list() {
                let opt = &opts[o.as_u64().unwrap() as usize];
                for c in &sets[row[2].as_u64().unwrap() as usize] {
                    assert_eq!(value(row[1].s(), opt, &ctxs[*c]), None, "{} {opt}", row[1]);
                    checked += 1;
                }
            }
        }
        let _game = games::playing("sts1");
        for call in g["calls"].list() {
            let (opt, ctx) = (&opts[call[1].as_u64().unwrap() as usize], &ctxs[call[2].as_u64().unwrap() as usize]);
            assert_eq!(value(call[0].s(), opt, ctx), call[3].as_f64(), "{call}");
            checked += 1;
        }
        let lost_ctxs: Vec<usize> = g["lost_ctxs"].list().iter().map(|n| n.as_u64().unwrap() as usize).collect();
        for row in g["lost"].list() {
            let got: Vec<f64> = lost_ctxs.iter().map(|c| _lost_card(row[0].s(), &ctxs[*c])).collect();
            let want: Vec<f64> = row[1].list().iter().map(|x| x.as_f64().unwrap()).collect();
            assert_eq!(got, want, "_lost_card {}", row[0]);
            checked += got.len();
        }
        let shares: Vec<f64> = g["shares"].list().iter().map(|x| x.as_f64().unwrap()).collect();
        for row in g["fight"].list() {
            let ctx = &ctxs[row[0].as_u64().unwrap() as usize];
            let want: Vec<f64> = row[1].list().iter().map(|x| x.as_f64().unwrap()).collect();
            assert_eq!(shares.iter().map(|s| _fight(*s, ctx)).collect::<Vec<_>>(), want, "_fight {}", row[0]);
            checked += shares.len();
        }
        for row in g["text"].list() {
            let (title, text) = _text(&row[0]);
            assert_eq!(json!([title, text]), row[1], "_text {}", row[0]);
            checked += 1;
        }
        assert!(checked > 270000 && numbers > 30000, "{checked} golden cases, {numbers} numbers");
    }

    #[test]
    fn tables_as_recorded() {
        let g: Value = serde_json::from_str(include_str!("../../../tests/golden/events_sts1.json")).unwrap();
        assert_eq!(json!(RULES.keys().collect::<Vec<_>>()), g["rules"]);
        let checked: serde_json::Map<String, Value> = GENERIC_CHECKED.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
        assert_eq!(base::json_dumps(&Value::Object(checked)), base::json_dumps(&g["generic_checked"]));
        assert_eq!(json!(COVERED.iter().collect::<Vec<_>>()), g["covered"]);
        assert_eq!((BASE, RUN), (g["base"].as_f64().unwrap(), g["run"].as_f64().unwrap()));
    }
}
