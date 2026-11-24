"""The Ironclad's cards, as the planner reads them ("6(9)": 6, or 9 once upgraded; "a||b": text a, or b upgraded)."""

CARDS = {
    "STRIKE_R": "Deal 6(9) damage.", "DEFEND_R": "Gain 5(8) Block.", "BASH": "Deal 8(10) damage. Apply 2(3) Vulnerable.",
    # commons
    "ANGER": "Deal 6(8) damage. Add a copy of this card into your discard pile.", "ARMAMENTS": "Gain 5 Block. Upgrade a card in your hand for the rest of combat.||Gain 5 Block. Upgrade ALL cards in your hand for the rest of combat.",
    "BODY_SLAM": "Deal damage equal to your Block.", "CLASH": "Can only be played if every card in your hand is an Attack. Deal 14(18) damage.", "CLEAVE": "Deal 8(11) damage to ALL enemies.", "CLOTHESLINE": "Deal 12(14) damage. Apply 2(3) Weak.",
    "FLEX": "Gain 2(4) Strength this turn.", "HAVOC": "Play the top card of your draw pile and Exhaust it.", "HEADBUTT": "Deal 9(12) damage. Place a card from your discard pile on top of your draw pile.",
    "HEAVY_BLADE": "Deal 14 damage. Strength affects this card 3(5) times.", "IRON_WAVE": "Gain 5(7) Block. Deal 5(7) damage.", "PERFECTED_STRIKE": "Deal 6 damage. Deals 2(3) additional damage for ALL your cards containing Strike.",
    "POMMEL_STRIKE": "Deal 9(10) damage. Draw 1(2) cards.", "SHRUG_IT_OFF": "Gain 8(11) Block. Draw 1 card.", "SWORD_BOOMERANG": "Deal 3 damage to a random enemy 3(4) times.", "THUNDERCLAP": "Deal 4(7) damage and apply 1 Vulnerable to ALL enemies.",
    "TRUE_GRIT": "Gain 7(9) Block. Exhaust a random card from your hand.||Gain 9 Block. Exhaust a card from your hand.", "TWIN_STRIKE": "Deal 5(7) damage twice.",
    "WARCRY": "Draw 1(2) cards. Put a card from your hand onto the top of your draw pile. Exhaust.", "WILD_STRIKE": "Deal 12(17) damage. Shuffle a Wound into your draw pile.",
    # uncommons
    "BATTLE_TRANCE": "Draw 3(4) cards. You cannot draw additional cards this turn.", "BLOOD_FOR_BLOOD": "Costs 1 less Energy for each time you lose HP in combat. Deal 18(22) damage.", "BLOODLETTING": "Lose 3 HP. Gain 2(3) Energy.",
    "BURNING_PACT": "Exhaust 1 card. Draw 2(3) cards.", "CARNAGE": "Ethereal. Deal 20(28) damage.", "COMBUST": "At the end of your turn, lose 1 HP and deal 5(7) damage to ALL enemies.", "DARK_EMBRACE": "Whenever a card is Exhausted, draw 1 card.",
    "DISARM": "Enemy loses 2(3) Strength. Exhaust.", "DROPKICK": "Deal 5(8) damage. If the enemy is Vulnerable, gain 1 Energy and draw 1 card.", "DUAL_WIELD": "Create 1(2) copy of an Attack or Power card in your hand.",
    "ENTRENCH": "Double your Block.", "EVOLVE": "Whenever you draw a Status card, draw 1(2) card.", "FEEL_NO_PAIN": "Whenever a card is Exhausted, gain 3(4) Block.",
    "FIRE_BREATHING": "Whenever you draw a Status or Curse card, deal 6(10) damage to ALL enemies.", "FLAME_BARRIER": "Gain 12(16) Block. Whenever you are attacked this turn, deal 4(6) damage back.", "GHOSTLY_ARMOR": "Ethereal. Gain 10(13) Block.",
    "HEMOKINESIS": "Lose 2 HP. Deal 15(20) damage.", "INFERNAL_BLADE": "Add a random Attack into your hand. It costs 0 this turn. Exhaust.", "INFLAME": "Gain 2(3) Strength.", "INTIMIDATE": "Apply 1(2) Weak to ALL enemies. Exhaust.",
    "METALLICIZE": "At the end of your turn, gain 3(4) Block.", "POWER_THROUGH": "Add 2 Wounds into your hand. Gain 15(20) Block.", "PUMMEL": "Deal 2 damage 4(5) times. Exhaust.", "RAGE": "Whenever you play an Attack this turn, gain 3(5) Block.",
    "RAMPAGE": "Deal 8 damage. Every time this card is played, increase its damage by 5(8) for this combat.", "RECKLESS_CHARGE": "Deal 7(10) damage. Shuffle a Dazed into your draw pile.",
    "RUPTURE": "Whenever you lose HP from a card, gain 1(2) Strength.", "SEARING_BLOW": "Deal 12(16) damage. Can be upgraded any number of times.", "SECOND_WIND": "Exhaust all non-Attack cards in your hand. Gain 5(7) Block for each card Exhausted.",
    "SEEING_RED": "Gain 2 Energy. Exhaust.", "SENTINEL": "Gain 5(8) Block. If this card is Exhausted, gain 2(3) Energy.", "SEVER_SOUL": "Exhaust all non-Attack cards in your hand. Deal 16(22) damage.",
    "SHOCKWAVE": "Apply 3(5) Weak and Vulnerable to ALL enemies. Exhaust.", "SPOT_WEAKNESS": "If the enemy intends to attack, gain 3(4) Strength.", "UPPERCUT": "Deal 13 damage. Apply 1(2) Weak. Apply 1(2) Vulnerable.",
    "WHIRLWIND": "Deal 5(8) damage to ALL enemies X times.",
    # rares
    "BARRICADE": "Block is not removed at the start of your turn.", "BERSERK": "Gain 2(1) Vulnerable. At the start of your turn, gain 1 Energy.", "BLUDGEON": "Deal 32(42) damage.", "BRUTALITY": "At the start of your turn, lose 1 HP and draw 1 card.",
    "CORRUPTION": "Skills cost 0. Whenever you play a Skill, Exhaust it.", "DEMON_FORM": "At the start of each turn, gain 2(3) Strength.", "DOUBLE_TAP": "This turn, your next 1(2) Attack is played twice.",
    "EXHUME": "Put a card from your exhaust pile into your hand. Exhaust.", "FEED": "Deal 10(12) damage. If Fatal, raise your Max HP by 3(4). Exhaust.",
    "FIEND_FIRE": "Exhaust all cards in your hand. Deal 7(10) damage for each Exhausted card. Exhaust.", "IMMOLATE": "Deal 21(28) damage to ALL enemies. Add a Burn into your discard pile.", "IMPERVIOUS": "Gain 30(40) Block. Exhaust.",
    "JUGGERNAUT": "Whenever you gain Block, deal 5(7) damage to a random enemy.", "LIMIT_BREAK": "Double your Strength. Exhaust.||Double your Strength.", "OFFERING": "Lose 6 HP. Gain 2 Energy. Draw 3(5) cards. Exhaust.",
    "REAPER": "Deal 4(5) damage to ALL enemies. Heal HP equal to unblocked damage. Exhaust.",
}
