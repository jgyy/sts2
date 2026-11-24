"""The Silent's cards, as the planner reads them ("6(9)": 6, or 9 once upgraded; "a||b": text a, or b upgraded)."""

CARDS = {
    "STRIKE_G": "Deal 6(9) damage.", "DEFEND_G": "Gain 5(8) Block.", "NEUTRALIZE": "Deal 3(4) damage. Apply 1(2) Weak.", "SURVIVOR": "Gain 8(11) Block. Discard 1 card.",
    # commons
    "ACROBATICS": "Draw 3(4) cards. Discard 1 card.", "BACKFLIP": "Gain 5(8) Block. Draw 2 cards.", "BANE": "Deal 7(10) damage. If the enemy has Poison, deal 7(10) damage again.", "BLADE_DANCE": "Add 3(4) Shivs into your hand.",
    "CLOAK_AND_DAGGER": "Gain 6 Block. Add 1(2) Shiv into your hand.", "DAGGER_SPRAY": "Deal 4(6) damage to ALL enemies twice.", "DAGGER_THROW": "Deal 9(12) damage. Draw 1 card. Discard 1 card.", "DEADLY_POISON": "Apply 5(7) Poison.",
    "DEFLECT": "Gain 4(7) Block.", "DODGE_AND_ROLL": "Gain 4(6) Block. Next turn, gain 4(6) Block.", "FLYING_KNEE": "Deal 8(11) damage. Next turn, gain 1 Energy.", "OUTMANEUVER": "Next turn, gain 2(3) Energy.",
    "PIERCING_WAIL": "ALL enemies lose 6(8) Strength this turn. Exhaust.", "POISONED_STAB": "Deal 6(8) damage. Apply 3(4) Poison.", "PREPARED": "Draw 1(2) card. Discard 1(2) card.", "QUICK_SLASH": "Deal 8(12) damage. Draw 1 card.",
    "SLICE": "Deal 6(9) damage.", "SNEAKY_STRIKE": "Deal 12(16) damage. If you have discarded a card this turn, gain 2 Energy.", "SUCKER_PUNCH": "Deal 7(9) damage. Apply 1(2) Weak.",
    # uncommons
    "ACCURACY": "Shivs deal 4(6) additional damage.", "ALL_OUT_ATTACK": "Deal 10(14) damage to ALL enemies. Discard 1 card at random.", "BACKSTAB": "Innate. Deal 11(15) damage. Exhaust.",
    "BLUR": "Gain 5(8) Block. Block is not removed at the start of your next turn.", "BOUNCING_FLASK": "Apply 3 Poison to a random enemy 3(4) times.",
    "CALCULATED_GAMBLE": "Discard your hand, then draw that many cards. Exhaust.||Discard your hand, then draw that many cards.", "CALTROPS": "Whenever you are attacked, deal 3(5) damage back.",
    "CATALYST": "Double an enemy's Poison. Exhaust.||Triple an enemy's Poison. Exhaust.", "CHOKE": "Deal 12 damage. Whenever you play a card this turn, the enemy loses 3(5) HP.", "CONCENTRATE": "Discard 3(2) cards. Gain 2 Energy.",
    "CRIPPLING_CLOUD": "Apply 4(7) Poison and 2 Weak to ALL enemies. Exhaust.", "DASH": "Gain 10(13) Block. Deal 10(13) damage.", "DISTRACTION": "Add a random Skill into your hand. It costs 0 this turn. Exhaust.",
    "ENDLESS_AGONY": "Whenever you draw this card, add a copy of it into your hand. Deal 4(6) damage. Exhaust.", "ESCAPE_PLAN": "Draw 1 card. If you draw a Skill, gain 3(5) Block.",
    "EVISCERATE": "Costs 1 less Energy for each card discarded this turn. Deal 7(9) damage three times.", "EXPERTISE": "Draw cards until you have 6(7) in your hand.", "FINISHER": "Deal 6(8) damage for each Attack played this turn.",
    "FLECHETTES": "Deal 4(6) damage for each Skill in your hand.", "FOOTWORK": "Gain 2(3) Dexterity.", "HEEL_HOOK": "Deal 5(8) damage. If the enemy is Weak, gain 1 Energy and draw 1 card.",
    "INFINITE_BLADES": "At the start of your turn, add a Shiv into your hand.", "LEG_SWEEP": "Apply 2(3) Weak. Gain 11(14) Block.", "MASTERFUL_STAB": "Costs 1 additional Energy for each time you lose HP this combat. Deal 12(16) damage.",
    "NOXIOUS_FUMES": "At the start of your turn, apply 2(3) Poison to ALL enemies.", "PREDATOR": "Deal 15(20) damage. Next turn, draw 2 cards.", "REFLEX": "Unplayable. If this card is discarded from your hand, draw 2(3) cards.",
    "RIDDLE_WITH_HOLES": "Deal 3(4) damage 5 times.", "SETUP": "Put a card from your hand on top of your draw pile. It costs 0 until played.", "SKEWER": "Deal 7(10) damage X times.",
    "TACTICIAN": "Unplayable. If this card is discarded from your hand, gain 1(2) Energy.", "TERROR": "Apply 99 Vulnerable. Exhaust.", "WELL_LAID_PLANS": "At the end of your turn, Retain up to 1(2) card.",
    # rares
    "A_THOUSAND_CUTS": "Whenever you play a card, deal 1(2) damage to ALL enemies.", "ADRENALINE": "Gain 1(2) Energy. Draw 2 cards. Exhaust.", "AFTER_IMAGE": "Whenever you play a card, gain 1 Block.", "ALCHEMIZE": "Obtain a random potion. Exhaust.",
    "BULLET_TIME": "You cannot draw any cards this turn. Reduce the cost of all cards in your hand to 0 this turn.", "BURST": "This turn, your next 1(2) Skill is played twice.",
    "CORPSE_EXPLOSION": "Apply 6(9) Poison. When the enemy dies, deal damage equal to its Max HP to ALL enemies.", "DIE_DIE_DIE": "Deal 13(17) damage to ALL enemies. Exhaust.", "DOPPELGANGER": "Next turn, draw X cards and gain X Energy. Exhaust.",
    "ENVENOM": "Whenever an Attack deals unblocked damage, apply 1 Poison.", "GLASS_KNIFE": "Deal 8(12) damage twice. Decrease the damage of this card by 2 this combat.",
    "GRAND_FINALE": "Can only be played if there are no cards in your draw pile. Deal 50(60) damage to ALL enemies.", "MALAISE": "Enemy loses X Strength. Apply X Weak. Exhaust.",
    "NIGHTMARE": "Choose a card. Next turn, add 3 copies of that card into your hand. Exhaust.", "PHANTASMAL_KILLER": "Next turn, your Attacks deal double damage.",
    "STORM_OF_STEEL": "Discard your hand. Add 1 Shiv into your hand for each card discarded.", "TOOLS_OF_THE_TRADE": "At the start of your turn, draw 1 card and discard 1 card.",
    "UNLOAD": "Deal 14(18) damage. Discard all non-Attack cards in your hand.", "WRAITH_FORM": "Gain 2(3) Intangible. At the end of your turn, lose 1 Dexterity.",
}
