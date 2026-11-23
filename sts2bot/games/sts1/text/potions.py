"""The first game's potions and the few relics whose text the bot reads, in words the bot's regexes know."""

POTIONS = {
    "AMBROSIA": "Enter Divinity.",
    "ANCIENT_POTION": "Gain 1 Artifact.",
    "ATTACK_POTION": "Choose 1 of 3 random Attack cards to add to your hand. It costs 0 this turn.",
    "BLESSING_OF_THE_FORGE": "Upgrade all cards in your hand for the rest of combat.",
    "BLOCK_POTION": "Gain 12 Block.",
    "BLOOD_POTION": "Heal 20% of your Max HP.",
    "BOTTLED_MIRACLE": "Add 2 Miracles to your hand.",
    "COLORLESS_POTION": "Choose 1 of 3 random Colorless cards to add to your hand. It costs 0 this turn.",
    "CULTIST_POTION": "Gain 1 Ritual.",
    "CUNNING_POTION": "Add 3 Upgraded Shivs to your hand.",
    "DEXTERITY_POTION": "Gain 2 Dexterity.",
    "DISTILLED_CHAOS": "Play the top 3 cards of your draw pile.",
    "DUPLICATION_POTION": "This turn, your next card is played twice.",
    "ELIXIR": "Exhaust any number of cards in your hand.",
    "ENERGY_POTION": "Gain 2 Energy.",
    "ENTROPIC_BREW": "Fill all your empty potion slots with random potions.",
    "ESSENCE_OF_DARKNESS": "Channel 1 Dark for each Orb slot.",
    "ESSENCE_OF_STEEL": "Gain 4 Plated Armor.",
    "EXPLOSIVE_POTION": "Deal 10 damage to ALL enemies.",
    "FAIRY_IN_A_BOTTLE": "When you would die, heal to 30% of your Max HP instead and discard this potion.",
    "FEAR_POTION": "Apply 3 Vulnerable.",
    "FIRE_POTION": "Deal 20 damage.",
    "FLEX_POTION": "Gain 5 Strength. At the end of your turn, lose 5 Strength.",
    "FOCUS_POTION": "Gain 2 Focus.",
    "FRUIT_JUICE": "Gain 5 Max HP.",
    "GAMBLER_S_BREW": "Discard any number of cards, then draw that many.",
    "GHOST_IN_A_JAR": "Gain 1 Intangible.",
    "HEART_OF_IRON": "Gain 6 Metallicize.",
    "LIQUID_BRONZE": "Gain 3 Thorns.",
    "LIQUID_MEMORIES": "Choose a card in your discard pile and return it to your hand. It costs 0 this turn.",
    "POISON_POTION": "Apply 6 Poison.",
    "POTION_OF_CAPACITY": "Gain 2 Orb slots.",
    "POWER_POTION": "Choose 1 of 3 random Power cards to add to your hand. It costs 0 this turn.",
    "REGEN_POTION": "Gain 5 Regen.",
    "SKILL_POTION": "Choose 1 of 3 random Skill cards to add to your hand. It costs 0 this turn.",
    "SMOKE_BOMB": "Escape from a non-boss combat.",
    "SNECKO_OIL": "Draw 5 cards. Randomize the cost of cards in your hand for the rest of combat.",
    "SPEED_POTION": "Gain 5 Dexterity. At the end of your turn, lose 5 Dexterity.",
    "STANCE_POTION": "Enter Calm or Wrath.",
    "STRENGTH_POTION": "Gain 2 Strength.",
    "SWIFT_POTION": "Draw 3 cards.",
    "WEAK_POTION": "Apply 3 Weak.",
}

# Potions the bot must never drink on its own: Fairy in a Bottle works from the belt, a Smoke Bomb ends a fight unwon.
KEEP = {"FAIRY_IN_A_BOTTLE", "SMOKE_BOMB"}

RELICS = {
    "SOZU": "Gain 1 Energy at the start of each turn. You can no longer obtain potions.",
    "RUNIC_DOME": "Gain 1 Energy at the start of each turn. You can no longer see enemy Intents.",
}

# Boss relics, best first for a bot that plans one turn at a time from what the enemies intend.
# Ones not listed come after these and before the ones in AVOID.
BOSS_RELICS = [
    "BLACK_BLOOD", "RING_OF_THE_SERPENT", "FROZEN_CORE", "HOLY_WATER", "PHILOSOPHER_S_STONE", "MARK_OF_PAIN",
    "BUSTED_CROWN", "FUSION_HAMMER", "ECTOPLASM", "VELVET_CHOKER", "SLAVER_S_COLLAR", "SOZU", "NUCLEAR_BATTERY",
    "INSERTER", "RUNIC_PYRAMID", "SACRED_BARK", "BLACK_STAR", "SNECKO_EYE", "WHITE_BEAST_STATUE", "TINY_HOUSE",
    "EMPTY_CAGE", "ASTROLABE", "CALLING_BELL", "PANDORA_S_BOX", "ORRERY",
]
AVOID = ["CURSED_KEY", "COFFEE_DRIPPER", "RUNIC_DOME"]  # (curses at every chest; no healing at rest sites; no intents to plan by)
