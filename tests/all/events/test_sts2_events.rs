//! Slay the Spire 2's events one by one (src/run/events/sts2.rs): the choice each rule makes, and that every event of the game
//! is covered by a rule or was checked against the generic scorer.

use serde_json::{json, Value};
use sts2bot::games;
use sts2bot::run::events::{self, sts2, EventOptionValueArgs};
use sts2bot::run::strategy;

// Every event with options in the game's localization/eng/events.json (v0.113.1), 2026-11-26.
const GAME_EVENTS: &str = "ABYSSAL_BATHS AMALGAMATOR AROMA_OF_CHAOS BATTLEWORN_DUMMY BRAIN_LEECH BUGSLAYER BYRDONIS_NEST COLORFUL_PHILOSOPHERS COLOSSAL_FLOWER
CRYSTAL_SPHERE DENSE_VEGETATION DOLL_ROOM DOORS_OF_LIGHT_AND_DARK DROWNING_BEACON ENDLESS_CONVEYOR FIELD_OF_MAN_SIZED_HOLES GRAVE_OF_THE_FORGOTTEN
HUNGRY_FOR_MUSHROOMS INFESTED_AUTOMATON JUNGLE_MAZE_ADVENTURE LOST_WISP LUMINOUS_CHOIR MOCK_EVENT_MODEL MORPHIC_GROVE MUSEUM_OF_TORTURE POTION_COURIER
PUNCH_OFF RANWID_THE_ELDER REFLECTIONS RELIC_TRADER ROOM_FULL_OF_CHEESE ROUND_TEA_PARTY SAPPHIRE_SEED SELF_HELP_BOOK SHRINE_OF_THE_WARLORD SLIPPERY_BRIDGE
SPIRALING_WHIRLPOOL SPIRIT_GRAFTER STONE_OF_ALL_TIME SUNKEN_STATUE SUNKEN_TREASURY SYMBIOTE TABLET_OF_TRUTH TEA_MASTER THERMAE_NONUPEIPE
THE_FUTURE_OF_POTIONS THE_LANTERN_KEY THE_LEGENDS_WERE_TRUE THIS_OR_THAT TINKER_TIME TRASH_HEAP TRIAL UNREST_SITE WAR_HISTORIAN_REPY
WATERFALL_OF_OBVIOUS_SECRETS WATERLOGGED_SCRIPTORIUM WELCOME_TO_WONGOS WELLSPRING WHISPERING_HOLLOW WOOD_CARVINGS WUPPY_IN_TROUBLE ZEN_WEAVER";

fn STRIKE() -> Value {
    json!({"name": "Strike", "id": "STRIKE_IRONCLAD", "type": "Attack", "cost": "1", "description": "Deal 6 damage."})
}

fn DEFEND() -> Value {
    json!({"name": "Defend", "id": "DEFEND_IRONCLAD", "type": "Skill", "cost": "1", "description": "Gain 5 Block."})
}

fn BASH() -> Value {
    json!({"name": "Bash", "id": "BASH", "type": "Attack", "cost": "2", "description": "Deal 8 damage. Apply 2 Vulnerable."})
}

fn STARTER() -> Vec<Value> {
    let mut deck = vec![STRIKE(); 5];
    deck.extend(vec![DEFEND(); 4]);
    deck.push(BASH());
    deck
}

/// The keyword arguments of the original's `pick`, with its defaults.
struct Pick {
    hp: i64,
    max_hp: i64,
    gold: i64,
    act: i64,
    floor: i64,
    deck: Vec<Value>,
    potions: Vec<Value>,
    slots: i64,
}

impl Default for Pick {
    fn default() -> Self {
        Pick { hp: 60, max_hp: 80, gold: 100, act: 1, floor: 5, deck: STARTER(), potions: Vec::new(), slots: 2 }
    }
}

/// The title of the option the bot takes.
fn pick(event_id: &str, options: &[(&str, &str)], p: Pick) -> String {
    let _game = games::playing("sts2");
    let mut mem = strategy::Memory::new();
    mem.deck = p.deck;
    let opts: Vec<Value> = options.iter().enumerate().map(|(i, (t, d))| json!({"index": i, "title": t, "description": d})).collect();
    let state = json!({"state_type": "event", "run": {"act": p.act, "floor": p.floor}, "event": {"event_id": event_id, "options": opts},
                       "player": {"hp": p.hp, "max_hp": p.max_hp, "gold": p.gold, "potions": p.potions, "relics": [], "max_potion_slots": p.slots}});
    let index = events::event(&state, &mut mem).into_iter().find(|a| a.0 == "choose_event_option").expect("an option chosen").1["index"].as_u64().unwrap();
    opts[index as usize]["title"].as_str().unwrap().to_string()
}

// EveryEvent

#[test]
fn test_every_event_of_the_game_has_a_rule_or_was_checked() {
    let missing: Vec<&str> = GAME_EVENTS.split_whitespace().filter(|e| !sts2::COVERED.contains(e)).collect();
    assert_eq!(missing, Vec::<&str>::new());
}

// Sts2Events

#[test]
fn test_battleworn_dummy_is_fought_where_it_can_be_killed() {
    // 0 of 22 logged fights killed the 300 HP dummy (125 dealt on average): nothing was ever won.
    let opts = [
        ("Setting 3", "Fight a 300 HP dummy. Obtain a random Relic."),
        ("Setting 2", "Fight a 150 HP dummy. Upgrade 2 random cards."),
        ("Setting 1", "Fight a 75 HP dummy. Procure 1 random Potion."),
    ];
    assert_eq!(pick("BATTLEWORN_DUMMY", &opts, Pick::default()), "Setting 1");
    let big = vec![json!({"name": "Bludgeon", "type": "Attack", "cost": "1", "description": "Deal 40 damage."}); 12];
    assert_eq!(pick("BATTLEWORN_DUMMY", &opts, Pick { deck: big, ..Pick::default() }), "Setting 3"); // (120 a turn)
}

#[test]
fn test_waterfall_beat_up_s_relic_beats_be_nice_s_card() {
    let opts = [("Be Nice", "Obtain Dingy Dagger."), ("Beat Up", "Obtain a random relic. Add 2 Guilty to your Deck.")];
    assert_eq!(pick("WATERFALL_OF_OBVIOUS_SECRETS", &opts, Pick::default()), "Beat Up");
}

#[test]
fn test_amalgamator_combines_strikes() {
    let opts = [("Combine Defends", "Remove 2 Defends. Add Ultimate Defend to your Deck."), ("Combine Strikes", "Remove 2 Strikes. Add Ultimate Strike to your Deck.")];
    assert_eq!(pick("AMALGAMATOR", &opts, Pick::default()), "Combine Strikes");
}

#[test]
fn test_colorful_philosophers_takes_cards_that_work_alone() {
    let opts = [("Pink", "Obtain 3 Necrobinder cards."), ("Orange", "Obtain 3 Regent cards."), ("Green", "Obtain 3 Silent cards.")];
    assert_eq!(pick("COLORFUL_PHILOSOPHERS", &opts, Pick::default()), "Green");
}

#[test]
fn test_colossal_flower_goes_down_for_its_relic_with_hp_to_spare() {
    let opts = [("Extract Nectar", "Gain 35 Gold."), ("Reach Deeper", "Enter deeper. Lose 5 HP.")];
    assert_eq!(pick("COLOSSAL_FLOWER", &opts, Pick { hp: 75, ..Pick::default() }), "Reach Deeper");
    assert_eq!(pick("COLOSSAL_FLOWER", &opts, Pick { hp: 20, ..Pick::default() }), "Extract Nectar");
    let centre = [("Extract Nectar", "Gain 140 Gold."), ("Enter the Center", "Lose 7 HP. Obtain Pollinous Core.")];
    assert_eq!(pick("COLOSSAL_FLOWER", &centre, Pick { hp: 60, ..Pick::default() }), "Enter the Center");
}

#[test]
fn test_doors_a_curse_is_better_gone_than_a_card_upgraded() {
    let opts = [("Light Door", "Upgrade a card."), ("Dark Door", "Remove 1 card from your Deck.")];
    let mut deck = STARTER();
    deck.push(json!({"name": "Regret", "type": "Curse"}));
    assert_eq!(pick("DOORS_OF_LIGHT_AND_DARK", &opts, Pick { deck, ..Pick::default() }), "Dark Door");
}

#[test]
fn test_losing_all_gold_is_not_the_end_of_the_run() {
    let opts = [("Loner", "Gain 5 Max HP."), ("Group", "Lose ALL of your Gold. Transform 2 cards.")];
    assert_eq!(pick("MORPHIC_GROVE", &opts, Pick { gold: 10, ..Pick::default() }), "Group");
    assert_eq!(pick("MORPHIC_GROVE", &opts, Pick { gold: 400, ..Pick::default() }), "Loner");
    let group = json!({"title": "Group", "description": "Lose ALL of your Gold. Transform 2 cards."});
    assert!(events::_event_option_value(&group, 60, 80, EventOptionValueArgs::default()) > -100.0);
}

#[test]
fn test_a_curse_named_curse_is_charged_once() {
    let take = events::_event_option_value(&json!({"title": "Take It", "description": "Obtain Thumbscrew. Add a random Curse to your Deck."}), 60, 80, EventOptionValueArgs::default());
    assert!(take > -1.0); // (was -6.5: the curse twice)
}

#[test]
fn test_sunken_statue_s_sword_wants_five_elites() {
    let opts = [("Grab the Sword", "Obtain the Sword of Stone."), ("Dive into the Water", "Gain 105 Gold. Lose 7 HP.")];
    assert_eq!(pick("SUNKEN_STATUE", &opts, Pick { hp: 40, ..Pick::default() }), "Dive into the Water");
}

#[test]
fn test_wuppy_soaks_more_for_an_injury() {
    let opts = [
        ("Be Cautious", "Lose 2 HP. Obtain Wuppy with 30 HP."),
        ("Help Immediately", "Add an Injury to your Deck. Obtain Wuppy with 60 HP."),
        ("Ignore", "Procure a random potion."),
    ];
    assert_eq!(pick("WUPPY_IN_TROUBLE", &opts, Pick::default()), "Help Immediately");
}

#[test]
fn test_round_tea_party_royal_poison_only_near_the_end() {
    let opts = [("Pick a Fight", "Lose 11 HP. Obtain a random Relic."), ("Enjoy Your Tea", "Obtain Royal Poison. Heal to full HP.")];
    assert_eq!(pick("ROUND_TEA_PARTY", &opts, Pick { hp: 60, floor: 20, act: 2, ..Pick::default() }), "Pick a Fight");
    assert_eq!(pick("ROUND_TEA_PARTY", &opts, Pick { hp: 20, floor: 45, act: 3, ..Pick::default() }), "Enjoy Your Tea");
}

#[test]
fn test_the_spoils_map_is_nothing_in_act_3() {
    let opts = [("Nab the Map", "Receive the Spoils Map."), ("Slowly Find an Exit", "Lose 8 HP. Procure 1 random Potion.")];
    assert_eq!(pick("THE_LEGENDS_WERE_TRUE", &opts, Pick { act: 1, ..Pick::default() }), "Nab the Map");
    assert_eq!(pick("THE_LEGENDS_WERE_TRUE", &opts, Pick { act: 3, floor: 40, hp: 70, ..Pick::default() }), "Slowly Find an Exit");
}

#[test]
fn test_wood_carvings_slither_only_for_dear_cards() {
    let opts = [("Snake", "Enchant 1 card with Slither."), ("Bird", "Choose 1 starter card to Transform into Peck.")];
    assert_eq!(pick("WOOD_CARVINGS", &opts, Pick::default()), "Bird");
}

#[test]
fn test_endless_conveyor_free_gold_is_taken() {
    let mut opts = [("Observe the Chef", "Upgrade a random card in your Deck."), ("Grab Golden Fysh off the Belt", "LUCKY WINNER! Gain 150 Gold.")];
    assert_eq!(pick("ENDLESS_CONVEYOR", &opts, Pick::default()), "Grab Golden Fysh off the Belt");
    opts[1] = ("Grab Seapunk Salad off the Belt", "Pay 40 Gold. Add Feeding Frenzy to your Deck. Continue feasting!");
    assert_eq!(pick("ENDLESS_CONVEYOR", &opts, Pick::default()), "Observe the Chef");
}

#[test]
fn test_trial_never_doubles_down() {
    assert_eq!(pick("TRIAL", &[("Accept", "Give in. Serve as today's Decider."), ("Double Down", "Face lethal repercussions.")], Pick::default()), "Accept");
    let merchant = [("DECIDE: Guilty", "Add Regret to your Deck. Obtain 2 random Relics."), ("DECIDE: Innocent", "Add Shame to your Deck. Upgrade 2 cards.")];
    assert_eq!(pick("TRIAL", &merchant, Pick::default()), "DECIDE: Guilty");
}

#[test]
fn test_the_lantern_key_is_returned_in_act_3() {
    let opts = [("Return the Key", "Gain 100 Gold."), ("Keep the Key", "Fight to obtain the Key.")];
    assert_eq!(pick("THE_LANTERN_KEY", &opts, Pick { act: 3, floor: 38, hp: 80, ..Pick::default() }), "Return the Key");
}

#[test]
fn test_unrest_site_poor_sleep_holds_a_place_in_every_hand() {
    let opts = [("Rest Anyways", "Heal to full HP. Receive Poor Sleep."), ("Kill the Trees", "Lose 8 Max HP. Obtain a random Relic.")];
    assert_eq!(pick("UNREST_SITE", &opts, Pick { hp: 70, ..Pick::default() }), "Kill the Trees");
}

#[test]
fn test_luminous_choir_two_removals_for_spore_mind() {
    let opts = [("Offer Tribute", "Pay 144 Gold. Obtain a random Relic."), ("Reach into the Flesh", "Remove 2 cards from your Deck. Add Spore Mind to your Deck.")];
    assert_eq!(pick("LUMINOUS_CHOIR", &opts, Pick { gold: 150, ..Pick::default() }), "Reach into the Flesh");
}

#[test]
fn test_a_rule_that_fails_leaves_the_option_to_the_generic_scorer() {
    let _game = games::playing("sts2");
    let ctx = events::context(&json!({"player": {"hp": 50, "max_hp": 80}, "run": {"act": 1, "floor": 3}}), &strategy::Memory::new(), 0);
    assert_eq!(events::rule_value("NOT_AN_EVENT", &json!({"title": "x"}), &ctx), None);
    assert_eq!(events::rule_value("ABYSSAL_BATHS", &json!({"title": null, "description": null}), &ctx), None);
}
