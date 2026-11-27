//! Colorless, Status and Curse cards of the first game, as the planner reads them ("6(9)": 6, or 9 once upgraded; "a||b": text a, or b upgraded).

use indexmap::IndexMap;
use std::sync::OnceLock;

#[rustfmt::skip]
pub static CARDS_ROWS: &[(&str, &str)] = &[
    // uncommon
    ("BANDAGE_UP", "Heal 4(6) HP. Exhaust."), ("BLIND", "Apply 2 Weak.||Apply 2 Weak to ALL enemies."), ("DARK_SHACKLES", "Enemy loses 9(15) Strength this turn. Exhaust."), ("DEEP_BREATH", "Shuffle your discard pile into your draw pile. Draw 1(2) card."),
    ("DISCOVERY", "Choose 1 of 3 random cards to add into your hand. It costs 0 this turn. Exhaust.||Choose 1 of 3 random cards to add into your hand. It costs 0 this turn."), ("DRAMATIC_ENTRANCE", "Innate. Deal 8(12) damage to ALL enemies. Exhaust."),
    ("ENLIGHTENMENT", "Reduce the cost of all cards in your hand to 1 this turn."), ("FINESSE", "Gain 2(4) Block. Draw 1 card."), ("FLASH_OF_STEEL", "Deal 3(6) damage. Draw 1 card."),
    ("FORETHOUGHT", "Put a card from your hand to the bottom of your draw pile. It costs 0 until played."), ("GOOD_INSTINCTS", "Gain 6(9) Block."), ("IMPATIENCE", "If you have no Attacks in your hand, draw 2(3) cards."),
    ("JACK_OF_ALL_TRADES", "Add 1(2) random Colorless card into your hand. Exhaust."), ("MADNESS", "A random card in your hand costs 0 for the rest of combat. Exhaust."),
    ("MIND_BLAST", "Innate. Deal damage equal to the number of cards in your draw pile."), ("PANACEA", "Gain 1(2) Artifact. Exhaust."), ("PANIC_BUTTON", "Gain 30(40) Block. You cannot gain Block from cards for 2 turns. Exhaust."),
    ("PURITY", "Exhaust up to 3(5) cards in your hand. Exhaust."), ("SWIFT_STRIKE", "Deal 7(10) damage."), ("TRIP", "Apply 2 Vulnerable.||Apply 2 Vulnerable to ALL enemies."),
    // rare
    ("APOTHEOSIS", "Upgrade ALL of your cards for the rest of combat. Exhaust."), ("CHRYSALIS", "Shuffle 3(5) random Skills into your draw pile. They cost 0 this combat. Exhaust."), ("HAND_OF_GREED", "Deal 20(25) damage. If Fatal, gain 20(25) Gold."),
    ("MAGNETISM", "At the start of each turn, add a random colorless card into your hand."), ("MASTER_OF_STRATEGY", "Draw 3(4) cards. Exhaust."), ("MAYHEM", "At the start of your turn, play the top card of your draw pile."),
    ("METAMORPHOSIS", "Shuffle 3(5) random Attacks into your draw pile. They cost 0 this combat. Exhaust."), ("PANACHE", "Every time you play 5 cards in a single turn, deal 10(14) damage to ALL enemies."),
    ("SADISTIC_NATURE", "Whenever you apply a Debuff to an enemy, they take 5(7) damage."), ("SECRET_TECHNIQUE", "Put a Skill from your draw pile into your hand. Exhaust."), ("SECRET_WEAPON", "Put an Attack from your draw pile into your hand. Exhaust."),
    ("THE_BOMB", "At the end of 3 turns, deal 40(50) damage to ALL enemies."), ("THINKING_AHEAD", "Draw 2 cards. Put a card from your hand on top of your draw pile. Exhaust.||Draw 2 cards. Put a card from your hand on top of your draw pile."),
    ("TRANSMUTATION", "Add X random colorless cards into your hand. They cost 0 this turn. Exhaust."), ("VIOLENCE", "Put 3(4) random Attacks from your draw pile into your hand. Exhaust."),
    // special (relics, events, other cards)
    ("APPARITION", "Ethereal. Gain 1 Intangible. Exhaust.||Gain 1 Intangible. Exhaust."), ("BITE", "Deal 7(8) damage. Heal 2(3) HP."), ("J_A_X", "Lose 3 HP. Gain 2(3) Strength."),
    ("RITUAL_DAGGER", "Deal 15 damage. If this kills an enemy, permanently increase this card's damage by 3(5). Exhaust."), ("SHIV", "Deal 4(6) damage. Exhaust."), ("SMITE", "Retain. Deal 12(16) damage. Exhaust."),
    ("SAFETY", "Retain. Gain 12(16) Block. Exhaust."), ("THROUGH_VIOLENCE", "Retain. Deal 20(30) damage. Exhaust."), ("INSIGHT", "Retain. Draw 2(3) cards. Exhaust."), ("MIRACLE", "Retain. Gain 1(2) Energy. Exhaust."),
    ("BETA", "Shuffle an Omega into your draw pile. Exhaust."), ("OMEGA", "At the end of your turn, deal 50(60) damage to ALL enemies."), ("EXPUNGER", "Deal 9(15) damage X times."), ("BECOME_ALMIGHTY", "Gain 3(4) Strength."),
    ("FAME_AND_FORTUNE", "Gain 25(30) Gold."), ("LIVE_FOREVER", "Gain 6(8) Plated Armor."),
    // status
    // (Burn, Decay and Regret in the second game's words, "if this is in your hand", which battle.hand_burns reads.)
    ("BURN", "Unplayable. At the end of your turn, if this is in your hand, take 2(4) damage."), ("DAZED", "Unplayable. Ethereal."), ("SLIMED", "Exhaust."), ("VOID", "Unplayable. Ethereal. Whenever this card is drawn, lose 1 Energy."),
    ("WOUND", "Unplayable."),
    // curses
    ("ASCENDER_S_BANE", "Unplayable. Ethereal. Cannot be removed from your deck."), ("CLUMSY", "Unplayable. Ethereal."), ("CURSE_OF_THE_BELL", "Unplayable. Cannot be removed from your deck."),
    ("DECAY", "Unplayable. At the end of your turn, if this is in your hand, take 2 damage."), ("DOUBT", "Unplayable. At the end of your turn, gain 1 Weak."), ("INJURY", "Unplayable."), ("NECRONOMICURSE", "Unplayable. There is no escape from this Curse."),
    ("NORMALITY", "Unplayable. You cannot play more than 3 cards this turn."), ("PAIN", "Unplayable. While in hand, lose 1 HP when other cards are played."), ("PARASITE", "Unplayable. If transformed or removed from your deck, lose 3 Max HP."),
    ("PRIDE", "Innate. At the end of your turn, put a copy of this card on top of your draw pile. Exhaust."), ("REGRET", "Unplayable. At the end of your turn, if this is in your hand, lose 1 HP for each card in your hand."),
    ("SHAME", "Unplayable. At the end of your turn, gain 1 Frail."), ("WRITHE", "Unplayable. Innate."),
];

/// `CARDS`, in the order of its rows.
#[allow(non_snake_case)]
pub fn CARDS() -> &'static IndexMap<&'static str, &'static str> {
    static TABLE: OnceLock<IndexMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| CARDS_ROWS.iter().copied().collect())
}
