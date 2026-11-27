//! Card understanding from the text the game shows on the card. There is no card database on purpose: the bot reads the
//! rendered description, so it keeps working when the game adds or rebalances cards.

/// A card's name or id as the tables spell it: DEMON_FORM for "Demon Form+".
pub fn key_of(name: &str) -> String {
    crate::re!(r"[^A-Z0-9]+").replace_all(name.to_uppercase().trim_end_matches('+'), "_").trim_matches('_').to_string()
}
