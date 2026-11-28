//! The planner's search: the turn a state shows (`_context`), every affordable line of the hand scored by `simulate`, and the
//! first card of the best one (`choose_card`), with the potions to drink around it (`actions`).

use super::*;

static LOG: Logger = Logger::new("sts2bot.combat");

thread_local! {
    // scripts/bosses/aeonfight.rs's WB=X: the planner's Wither bill times X while this is set
    // (the script's `mock.patch.object(combat, "_wither_bill", lambda *a: bill(*a) * value)`)
    static WITHER_BILL_TIMES: Cell<Option<f64>> = const { Cell::new(None) };
    static NEXT_ROOM: RefCell<Option<Rc<dyn Fn(&Value) -> (f64, f64)>>> = const { RefCell::new(None) };
    static CHOOSE_CARD: RefCell<Option<Rc<dyn Fn(&Value, Option<&mut Plan>) -> Option<Action>>>> = const { RefCell::new(None) };
}

/// The planner's Wither bill times `times` on this thread from now on (None: as battle.rs has it).
pub fn set_wither_bill_times(times: Option<f64>) -> Option<f64> {
    WITHER_BILL_TIMES.with(|t| t.replace(times))
}

/// battle.rs's `next_room`, or what a test has put in its place (`with_next_room`): `_context` asks here.
pub fn next_room(state: &Value) -> (f64, f64) {
    match NEXT_ROOM.with(|h| h.borrow().clone()) {
        Some(hook) => hook(state),
        None => battle::next_room(state),
    }
}

/// Puts the earlier `next_room` back when dropped.
#[must_use = "the override ends when this is dropped"]
pub struct NextRoomGuard(Option<Rc<dyn Fn(&Value) -> (f64, f64)>>);

impl Drop for NextRoomGuard {
    fn drop(&mut self) {
        NEXT_ROOM.with(|h| *h.borrow_mut() = self.0.take());
    }
}

/// `mock.patch.object(combat, "next_room", f)`: on this thread, until the guard is dropped.
pub fn with_next_room(f: impl Fn(&Value) -> (f64, f64) + 'static) -> NextRoomGuard {
    NextRoomGuard(NEXT_ROOM.with(|h| h.borrow_mut().replace(Rc::new(f))))
}

/// Puts the earlier `choose_card` back when dropped.
#[must_use = "the override ends when this is dropped"]
pub struct ChooseCardGuard(Option<Rc<dyn Fn(&Value, Option<&mut Plan>) -> Option<Action>>>);

impl Drop for ChooseCardGuard {
    fn drop(&mut self) {
        CHOOSE_CARD.with(|h| *h.borrow_mut() = self.0.take());
    }
}

/// `mock.patch.object(combat, "choose_card", ...)`: `f` answers every `combat::choose_card` on this thread (those of `actions` too)
/// until the guard is dropped. A spy (`wraps=combat.choose_card`) counts and calls `combat::_choose_card`, the planner itself.
pub fn with_choose_card_hook(f: impl Fn(&Value, Option<&mut Plan>) -> Option<Action> + 'static) -> ChooseCardGuard {
    ChooseCardGuard(CHOOSE_CARD.with(|h| h.borrow_mut().replace(Rc::new(f))))
}

/// HP the planner expects to lose if the turn ended now (logged, to catch what it does not model).
pub fn expected_end_loss(state: &Value) -> Option<i64> {
    let enemies = read_enemies(state);
    if enemies.is_empty() {
        return None;
    }
    Some(simulate::<CardInfo, _>(&[], &enemies, _context(state)).unwrap().lost) // (an empty line can always be afforded)
}

/// `int(x or default)`.
fn _int_or(x: &Value, default: i64) -> i64 {
    if x.truthy() { x.int() } else { default }
}

/// The keywords of `simulate` for the turn this state shows.
pub fn _context(state: &Value) -> SimulateArgs {
    let player = &state["player"];
    let battle = &state["battle"];
    let draws = _draw_worth(player);
    let foes = battle["enemies"].list();
    let round_no = _int_or(&battle["round"], 1);
    let fight = if state["state_type"].truthy() { state["state_type"].text() } else { "monster".to_string() };
    let mut burns = hand_burns(player);
    // (damage that Block stops)
    burns.extend([end_of_turn_loss(player), constrict(player)].into_iter().enumerate().filter(|(_, hp)| *hp != 0).map(|(n, hp)| (-1 - n as i64, hp, true)));
    let wither_hp = _next_wither(player, round_no);
    SimulateArgs {
        energy: player["energy"].int(),
        stars: player["stars"].int(),
        hp: 1.max(_int_or(&player["hp"], 1) - 0.max(power_amount(player, "DOOM")) - 0.max(power_amount(player, "NEUROSURGE"))), // (Doom on us kills once it reaches our HP)
        max_hp: _int_or(&player["max_hp"], 1),
        block: player["block"].int() + end_of_turn_block(player) + osty_hp(player),
        fortify: _int_or(&state["_fortify"], 1),
        round_no,
        card_limit: cards_left_this_turn(state),
        can_block: !_has_power(player, "NO_BLOCK"),
        sandpit: _sandpit(state),
        escape_odds: _escape_odds(player, 5),
        burns,
        hand_size: player["hand"].list().len() as i64,
        wither_in: foes.iter().map(|e| power_amount(e, "WITHERING_PRESENCE")).max().unwrap_or(0),
        wither_hp,
        wither_bill: match WITHER_BILL_TIMES.with(|t| t.get()) {
            Some(times) => _wither_bill(player, round_no, wither_hp) * times,
            None => _wither_bill(player, round_no, wither_hp),
        },
        draw_worth: draws,
        orbs: player["orbs"].list().iter().map(|o| (o["id"].or(&o["name"]).text().to_uppercase().replace("_ORB", ""), o["passive_val"].int(), o["evoke_val"].int())).collect(),
        orb_slots: player["orb_slots"].int(),
        bare_block: _bare_block(player),
        regen: 0.max(power_amount(player, "REGEN")),
        focus_power: if _has_power(player, "FOCUS") { power_amount(player, "FOCUS") } else { 0 },
        shiv_damage: 4 + 0.max(power_amount(player, "ACCURACY")),
        tender: _has_power(player, "TENDER"),
        hail: 0.max(power_amount(player, "HAILSTORM")),
        deck: deck_count(player),
        strip: if foes.iter().any(|e| _has_power(e, "ARTIFACT")) { artifact::outlook(player, &fight, round_no) } else { (0.0, 0.0) },
        vigor: 0.max(power_amount(player, "VIGOR")),
        giant: _has_power(player, "GIGANTIFICATION"),
        buffer: 0.max(power_amount(player, "BUFFER")),
        loss_cap: loss_cap(player),
        trig: triggers::read(state),
        pet: if player["pets"].truthy() { osty_hp(player) } else { -1 },
        mastery: 0.max(power_amount(player, "NECRO_MASTERY")),
        might0: state["_might"].int(),
        holds: _has_power(player, "BARRICADE"),
        friends: 0.max(power_amount(player, "FRIENDSHIP")),
        drained: 0.max(power_amount(player, "ENERGY_DOWN")),
        free: free_plays(state),
        doubled: _has_power(player, "DUPLICATION") || 0.max(power_amount(player, "ECHO_FORM")) > state["_plays_this_turn"].int(), // (Echo Form: the first card(s) a turn)
        taint: std::iter::once(player).chain(foes).map(|e| power_amount(e, "VITAL_SPARK")).max().unwrap_or(0),
        pace: closing::pace(state),
        room: next_room(state),
        arrows: arrows_owned(player, foes),
        aiming: 0.max(power_amount(player, "AIM")),
        stretch: 0.max(power_amount(player, "STRETCH")),
        tune: played_melodies(&state["_played_names"].list().iter().map(|name| name.text()).collect::<Vec<_>>()),
        chord: 0.max(power_amount(player, "VIOLENT_CHORD")),
        boom: 0.max(power_amount(player, "EXPLOSIVE_ARROWS")),
        fool: fool_powers(player, state["_dislodged"].truthy()),
        stance0: player["stance"].text(),
        wrath: _wrath_outlook(state),
        sadness: sad_hand::outlook(state, draws),
        mantra0: 0.max(power_amount(player, "MANTRA")),
        eruption: eruption::outlook(state),
        vulnerable: _has_power(player, "VULNERABLE"),
        fight,
        // (the rest: as `simulate` has them when they are not given)
        end_loss: 0,
        focus: None,
    }
}

/// For a Watcher who is in Wrath or holds a card that enters it: the chance that next turn's hand holds no way out, and each
/// enemy's blow on the turns it attacked, as it would be outside Wrath, with how often it attacked (`_blow`, kept by the bot). Else None.
pub fn _wrath_outlook(state: &Value) -> Option<Wrath> {
    let player = &state["player"];
    if player["stance"].is_null() {
        return None; // (a game without stances)
    }
    let hand: Vec<CardInfo> = player["hand"].list().iter().map(parse).collect();
    if read_stance(&player["stance"]) != "WRATH" && !hand.iter().any(|c| c.stance == "WRATH" || c.wrath_next) {
        return None;
    }
    let fury = hand.iter().filter(|c| c.wrath_next).map(|c| c.next_draw).max().unwrap_or(0);
    let blows = state["_blow"].entries().map(|(id, seen)| (id.clone(), (seen[0].float(), seen[1].float()))).collect();
    Some((stuck_odds(player, 0), blows, if fury != 0 { stuck_odds(player, fury) } else { 1.0 }))
}

/// What `choose_card` leaves for its caller: `lost`, the HP the best line still loses; its `score`; the `line` itself.
/// (the original's `plan` dict: a field is None while the key is not there.)
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Plan {
    pub lost: Option<i64>,
    pub score: Option<f64>,
    pub line: Option<Vec<CardInfo>>,
}

/// A line's cards as the memo knows them: each card's sort and whether it is aimed, and the aim (its place in `aims`; 0 for a line with nothing aimed).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum MemoKey {
    Packed(u128, usize),
    Long(Vec<(usize, bool)>, usize),
}

/// `itertools.permutations(range(n))` after the first: the next arrangement in its order, false after the last.
pub(crate) fn next_permutation(order: &mut [usize]) -> bool {
    let n = order.len();
    if n < 2 {
        return false;
    }
    let mut i = n - 1;
    while i > 0 && order[i - 1] >= order[i] {
        i -= 1;
    }
    if i == 0 {
        return false;
    }
    let mut j = n - 1;
    while order[j] <= order[i - 1] {
        j -= 1;
    }
    order.swap(i - 1, j);
    order[i..].reverse();
    true
}

/// `itertools.combinations(range(n), len(picked))` after the first: the next choice in its order, false after the last.
pub(crate) fn next_combination(picked: &mut [usize], n: usize) -> bool {
    let size = picked.len();
    let mut i = size;
    while i > 0 {
        i -= 1;
        if picked[i] != i + n - size {
            picked[i] += 1;
            for j in i + 1..size {
                picked[j] = picked[j - 1] + 1;
            }
            return true;
        }
    }
    false
}

/// Best card to play right now, or None to end the turn (`plan.lost`: HP the best line still loses).
/// (Answered by a test's stand-in while one is in place: `with_choose_card_hook`.)
pub fn choose_card(state: &Value, plan: Option<&mut Plan>) -> Option<Action> {
    match CHOOSE_CARD.with(|h| h.borrow().clone()) {
        Some(hook) => hook(state, plan),
        None => _choose_card(state, plan),
    }
}

/// `choose_card` itself, whatever stands in for it.
pub fn _choose_card(state: &Value, plan: Option<&mut Plan>) -> Option<Action> {
    let player = &state["player"];
    let enemies = read_enemies(state);
    let ctx = _context(state);
    let mut hand = triggers::counted(player["hand"].list().iter().map(parse).collect());
    let free = ctx.free;
    if free != 0 {
        // Void Form: with free plays left every card shows as costing 0; what each costs after them is what the deck said when the fight began.
        for c in hand.iter_mut() {
            if let Some(cost) = state["_costs"].get(c.name.as_str()) {
                c.cost = cost.int();
            }
        }
    }
    let hand = hand;
    let mut playable: Vec<&CardInfo> = hand.iter().filter(|c| c.playable && (c.is_x() || free != 0 || c.cost <= ctx.energy) && c.star_cost <= ctx.stars).collect();
    playable.retain(|c| !triggers::shunned(c, state));
    let owned = |cards: &[&CardInfo]| cards.iter().map(|c| (*c).clone()).collect::<Vec<CardInfo>>();
    if enemies.is_empty() && state["_revive_pending"].truthy() {
        return _set_up(&owned(&playable), ctx.energy);
    }
    if enemies.len() == 1 && enemies[0].asleep >= 2 && ctx.round_no <= 2 {
        // Asleep ("Awakens upon losing HP or after 3 turns"): she wakes on her own time whatever we do, so the turns before the last are for what lasts
        // (waking her early with a strong hand lost fights in the model: scripts bosses/matriarchfight, 68% -> 63%).
        return _set_up(&owned(&playable), ctx.energy);
    }
    if playable.is_empty() || enemies.is_empty() {
        if let (Some(plan), false) = (plan, enemies.is_empty()) {
            if let Some(base) = simulate::<CardInfo, _>(&[], &enemies, &ctx) {
                // (ending the turn is the plan: handpick prices a card by it)
                plan.lost = Some(base.lost);
                plan.score = Some(base.score);
                plan.line = Some(Vec::new());
            }
        }
        return None;
    }
    // Cards we cannot pay for yet can still come later in a line that first makes the energy or Stars for them (evoking a Plasma orb too).
    let orbs = &ctx.orbs;
    let pays = |c: &CardInfo| {
        c.energy_gain != 0
            || c.star_gain != 0
            || triggers::PAYS.contains(&c.key.as_str())
            || (c.key == "CHO_CHORUS" && battle::FOOL_READS.get())
            || (c.evoke != 0 && !orbs.is_empty() && orbs[0].0 == "PLASMA")
            || (c.evoke_all != 0 && orbs.iter().any(|o| o.0 == "PLASMA"))
    };
    let makers = playable.iter().any(|c| pays(c) || triggers::FREE_NEXT.contains_key(c.key.as_str()));
    if makers {
        for c in hand.iter() {
            if c.short && !playable.contains(&c) {
                playable.push(c);
            }
        }
    }
    let most = MAX_SEARCH_CARDS.get();
    if playable.len() as i64 > most {
        base::sort_by_key(&mut playable, |c| -(c.total_damage() + c.block + 3 * c.draw));
        playable.truncate(most.max(0) as usize);
    }
    if ctx.card_limit.is_some_and(|limit| limit <= 0) {
        return None;
    }
    if ctx.card_limit.is_some_and(|limit| limit <= 3) {
        base::sort_by_key(&mut playable, |c| (-(c.total_damage() + c.block), c.index)); // few plays allowed: spend them on what does the most now
    } else if enemies.iter().any(|e| e.slow >= 0) {
        base::sort_by_key(&mut playable, |c| (if c.damage != 0 { 1 } else { 0 }, c.total_damage(), c.index)); // Slow: every card played makes later attacks hit harder
    } else if enemies.iter().any(|e| e.slippery != 0) {
        base::sort_by_key(&mut playable, |c| (if c.damage == 0 { order_key(c).0 } else { 3 }, c.damage, c.index)); // Slippery: burn it off with the weakest hits
    } else {
        // (Flatten and Rattle after Osty's other attacks: one is free then, the other hits again. No Escape after the other Doom.)
        let after = |c: &CardInfo| -> (i64, f64, i64) {
            let key = order_key(c);
            if c.key == "FLATTEN" || c.key == "RATTLE" || c.key == "NO_ESCAPE" {
                (key.0, 1e3, c.index)
            } else if c.key == "DISLIKE_YOU" && battle::FOOL_READS.get() {
                (2, key.1 as f64, key.2) // (before the Shots it makes harder)
            } else {
                (key.0, key.1 as f64, key.2)
            }
        };
        base::sort_by_key(&mut playable, |c| {
            if pays(c) {
                let key = order_key(c);
                (1, key.1 as f64, key.2)
            } else {
                after(c)
            }
        });
    }
    // The free plays go to the dearest cards; a card that ends the turn (Void Form itself) goes last.
    let mut dear: Vec<&CardInfo> = playable.iter().copied().filter(|c| c.cost > 0).collect();
    base::sort_by_key(&mut dear, |c| -c.cost);
    dear.truncate(free.max(0) as usize);
    if ctx.doubled && !playable.is_empty() && !playable.iter().any(|c| c.r#type == "Power") {
        dear.push(base::max_by_key(playable.iter().copied(), |c| c.total_damage() + c.block + 3 * c.draw).unwrap()); // the card worth playing twice leads
    }
    base::sort_by_key(&mut playable, |c| (c.ends_turn, !dear.contains(c))); // (stable: the rest keep their order)
    // Bound cards: one per turn (once one is played the game marks the rest unplayable, so any still playable means it is unused).
    let bound_left: Option<i64> = if playable.iter().any(|c| c.bound) { Some(1) } else { None };
    let skill_left: Option<i64> = if _has_power(&state["player"], "SMOGGY") { Some(1) } else { None }; // Smoggy ("You can only play 1 Skill per turn") the same way
    // Copies of a card that differ only in their place in the hand, and a line with nothing aimed at every aim, play out alike: simulated once.
    let mut sorts: Vec<CardInfo> = Vec::new();
    let sort_of: Vec<(usize, bool)> = playable
        .iter()
        .map(|c| {
            let sort = if ctx.burns.iter().any(|b| b.0 == c.index) { (*c).clone() } else { CardInfo { index: -1, ..(*c).clone() } };
            let id = match sorts.iter().position(|known| *known == sort) {
                Some(id) => id,
                None => {
                    sorts.push(sort);
                    sorts.len() - 1
                }
            };
            (id, c.damage != 0 || c.needs_target || c.osty_attack || c.key == "TIMES_UP" || c.key == "TIME_S_UP")
        })
        .collect();
    let mut memo: HashMap<MemoKey, Option<Outcome>> = HashMap::new();
    let baseline = simulate::<CardInfo, _>(&[], &enemies, &ctx);
    let (mut best_score, mut best_lost) = match &baseline {
        Some(baseline) => (baseline.score, Some(baseline.lost)),
        None => (f64::NEG_INFINITY, None),
    };
    let mut best_line: Vec<&CardInfo> = Vec::new();
    let mut best_target: Option<Arc<str>> = None;
    // Each line is also tried aimed wholly at one enemy: when surrounded (whom we face decides who hits us from behind), and in any small group.
    let mut aims: Vec<Option<Focus>> = vec![None];
    aims.extend(enemies.iter().filter(|e| e.flanker).map(|e| Some(Focus::Last(e.entity_id.to_string()))));
    aims.extend(enemies.iter().filter(|e| e.flanker || (1 < enemies.len() && enemies.len() <= 3 && e.incoming > 0)).map(|e| Some(Focus::At(e.entity_id.to_string()))));
    // Order matters too (the Slow bonus, what an X card leaves): short lines are tried in every order, the usual one first so that it wins ties.
    let mut budget: i64 = 2500;
    let cards = playable.len();
    let packed = cards <= 16 && sorts.len() < 127;
    let mut order: Vec<&CardInfo> = Vec::with_capacity(cards);
    for size in 1..=cards {
        let mut found = false;
        let mut picked: Vec<usize> = (0..size).collect();
        // keeps order_key order
        loop {
            let line = &picked;
            let skip = bound_left.is_some_and(|left| line.iter().filter(|&&i| playable[i].bound).count() as i64 > left) // "Only 1 Bound card can be played each turn."
                || skill_left.is_some_and(|left| line.iter().filter(|&&i| playable[i].r#type == "Skill").count() as i64 > left);
            if !skip {
                let every_order = 1 < size && size <= 4;
                let mut places: Vec<usize> = (0..size).collect(); // starts with `line` itself
                let mut nth = 0;
                loop {
                    if nth != 0 && budget <= 0 {
                        break;
                    }
                    let mut usable = true;
                    let any_aimed = places.iter().any(|&p| sort_of[line[p]].1);
                    let sig: Option<u128> = if packed { Some(places.iter().fold(0u128, |sig, &p| (sig << 8) | (((sort_of[line[p]].0 as u128 + 1) << 1) | sort_of[line[p]].1 as u128))) } else { None };
                    order.clear();
                    order.extend(places.iter().map(|&p| playable[line[p]]));
                    for (at, aim) in aims.iter().enumerate() {
                        if nth != 0 {
                            budget -= 1;
                        }
                        let at = if any_aimed { at } else { 0 };
                        let key = match sig {
                            Some(sig) => MemoKey::Packed(sig, at),
                            None => MemoKey::Long(places.iter().map(|&p| sort_of[line[p]]).collect(), at),
                        };
                        let out = memo.entry(key).or_insert_with(|| simulate_aimed(&order, &enemies, &ctx, aim.as_ref()));
                        let Some(out) = out else {
                            usable = false;
                            break;
                        };
                        found = true;
                        if out.score > best_score + 1e-9 {
                            best_score = out.score;
                            best_line.clear();
                            best_line.extend(order.iter().copied());
                            best_target = out.first_target.clone();
                            best_lost = Some(out.lost);
                        }
                    }
                    if !usable && nth == 0 && !line.iter().any(|&i| playable[i].is_x() || pays(playable[i]) || playable[i].key == "FLATTEN" || triggers::FREE_NEXT.contains_key(playable[i].key.as_str())) {
                        break; // cannot be paid for in any order
                    }
                    nth += 1;
                    if !(every_order && next_permutation(&mut places)) {
                        break;
                    }
                }
            }
            if !next_combination(&mut picked, cards) {
                break;
            }
        }
        if !found {
            break;
        }
    }
    if let Some(plan) = plan {
        plan.lost = best_lost;
        plan.score = Some(best_score);
        plan.line = Some(best_line.iter().map(|c| (*c).clone()).collect());
    }
    if best_line.is_empty() {
        return None;
    }
    let card = best_line[0];
    let mut params = json!({"card_index": card.index});
    if card.needs_target {
        params["target"] = match best_target.filter(|id| !id.is_empty()) {
            Some(id) => json!(&*id),
            None => json!(&*base::min_by_key(enemies.iter(), |e| e.hp + e.block).unwrap().entity_id),
        };
    }
    Some(("play_card".to_string(), params))
}

/// Ordered candidate actions for a combat state (first workable one is taken).
pub fn actions(state: &Value) -> Vec<Action> {
    if LOG.enabled(Level::Debug) {
        crate::log_debug!(LOG, "combat {}", describe(state));
        if let Some(sandpit) = _sandpit(state) {
            let odds = _escape_odds(&state["player"], 5);
            crate::log_debug!(LOG, "sandpit {}, no Escape next hand {:.2}, later {:.2}", sandpit, odds.0, odds.1);
        }
    }
    let mut plan = Plan::default();
    let mut card = choose_card(state, Some(&mut plan));
    if plan.lost.unwrap_or(0) >= _int_or(&state["player"]["hp"], 1) && fortifier(&state["player"]) {
        // Dead on paper with a Fortifier on the belt: it is drunk after the last card (potions._ill_timed), so the cards are chosen for the Block it will triple.
        let mut braced = Plan::default();
        let mut fortified = state.clone();
        fortified["_fortify"] = json!(3);
        let with_it = choose_card(&fortified, Some(&mut braced));
        if braced.lost.or(plan.lost) < plan.lost {
            // (the potions still go by the loss without it: the danger it is drunk for)
            card = with_it;
            plan.line = Some(braced.line.unwrap_or_default());
        }
    }
    // (nothing playable: no plan)
    let planned_loss = if plan.lost.is_some() { plan.lost } else { expected_end_loss(state) };
    let mut out = potion_actions(state, planned_loss, Some(plan.line.as_deref().unwrap_or(&[])));
    if let Some(card) = card {
        out.push(card);
    }
    out.push(("end_turn".to_string(), json!({})));
    out
}
