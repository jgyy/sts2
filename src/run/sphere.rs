//! The Crystal Sphere (user, 2026-11-28: go for the items, not the middle): an 11x11 grid under fog and a few
//! divinations, each clearing 3x3 ("big") or one cell ("small"). An item is won only once every one of its cells is
//! clear (the curse too), and as soon as any cell of one shows, the state gives its whole rectangle.

use std::collections::HashSet;

use serde_json::{json, Value};

use crate::base::{self, JsonExt};
use crate::Action;

crate::tunables! {
    pub SPHERE_AIM: bool = true; // (false: the first version, from the middle outward; the strategy recordings use it)
}

const PROGRESS: f64 = 0.6; // what clearing part of an item that can still be finished counts for, as a share of its worth
const SEEK: f64 = 0.12; // a fogged cell cleared where nothing is known, while a later divination can use what it shows

type Cell = (i64, i64);

/// What winning an item is worth (a relic most; the curse costs).
fn worth(item: &Value) -> f64 {
    let cells = item["width"].int() * item["height"].int();
    match item["item_type"].as_str().unwrap_or("") {
        "CrystalSphereRelic" => 10.0,
        "CrystalSphereCardReward" => 4.0,
        "CrystalSpherePotion" if cells == 4 => 5.0, // (a rare one is 2x2, a common one 1x3)
        "CrystalSpherePotion" => 3.0,
        "CrystalSphereGold" if cells > 1 => 3.0,
        "CrystalSphereGold" => 1.0,
        "CrystalSphereCurse" => -8.0,
        _ if item["is_good"].truthy() => 3.0,
        _ => -8.0,
    }
}

/// The fewest 3x3 divinations that could cover these cells (by the rectangle around them).
fn squares(cells: &[Cell]) -> i64 {
    let span = |k: fn(&Cell) -> i64| cells.iter().map(k).max().unwrap_or(0) - cells.iter().map(k).min().unwrap_or(0) + 1;
    if cells.is_empty() {
        0
    } else {
        ((span(|c| c.0) + 2) / 3) * ((span(|c| c.1) + 2) / 3)
    }
}

/// Every (tool, cell) worth trying, best first.
fn aim(cs: &Value) -> Vec<(&'static str, Cell)> {
    let at = |v: &Value| (v["x"].int(), v["y"].int());
    let hidden: HashSet<Cell> = cs["cells"].list().iter().filter(|c| c["is_hidden"].truthy()).map(at).collect();
    let items: Vec<(Vec<Cell>, f64)> = cs["revealed_items"]
        .list()
        .iter()
        .map(|i| {
            let (x, y) = at(i);
            let cells = (x..x + i["width"].int()).flat_map(|x| (y..y + i["height"].int()).map(move |y| (x, y)));
            (cells.collect(), worth(i))
        })
        .collect();
    let known: HashSet<Cell> = items.iter().flat_map(|(cells, _)| cells.iter().copied()).collect();
    let digits: String = cs["divinations_left_text"].text().chars().filter(char::is_ascii_digit).collect();
    let left: i64 = digits.parse().unwrap_or(3);
    let (cx, cy) = (cs["grid_width"].int() / 2, cs["grid_height"].int() / 2);
    let tools: Vec<(&'static str, i64)> = [("big", 1), ("small", 0)].into_iter().filter(|(t, _)| cs[format!("can_use_{t}_tool").as_str()].truthy()).collect();
    let mut tries: Vec<(f64, i64, &'static str, Cell)> = Vec::new();
    for (tool, reach) in tools {
        for click in cs["clickable_cells"].list().iter().map(at) {
            let cleared = |c: &Cell| hidden.contains(c) && (c.0 - click.0).abs() <= reach && (c.1 - click.1).abs() <= reach;
            let mut score = 0.0;
            for (cells, value) in &items {
                let rest: Vec<Cell> = cells.iter().filter(|c| hidden.contains(c)).copied().collect();
                let after: Vec<Cell> = rest.iter().filter(|c| !cleared(c)).copied().collect();
                if after.len() == rest.len() {
                    continue;
                }
                let part = (rest.len() - after.len()) as f64 / rest.len() as f64;
                score += if after.is_empty() {
                    *value // (won, or the curse taken)
                } else if *value < 0.0 {
                    0.1 * value * part // (nearer to taking the curse)
                } else if squares(&after) < left {
                    PROGRESS * value * part
                } else {
                    0.0
                };
            }
            let unknown = (-reach..=reach).flat_map(|dx| (-reach..=reach).map(move |dy| (click.0 + dx, click.1 + dy))).filter(|c| hidden.contains(c) && !known.contains(c)).count();
            score += unknown as f64 * if left > 1 { SEEK } else { 0.02 };
            tries.push((score, (click.0 - cx).abs() + (click.1 - cy).abs(), tool, click));
        }
    }
    base::sort_by_key(&mut tries, |t| (-t.0, t.1));
    tries.into_iter().map(|t| (t.2, t.3)).collect()
}

pub fn crystal_sphere(state: &Value) -> Vec<Action> {
    let cs = &state["crystal_sphere"];
    if cs["can_proceed"].truthy() {
        return vec![("crystal_sphere_proceed".to_string(), json!({}))];
    }
    let click = |x: Value, y: Value| ("crystal_sphere_click_cell".to_string(), json!({"x": x, "y": y}));
    let tries = if SPHERE_AIM.get() { aim(cs) } else { Vec::new() };
    let mut out: Vec<Action> = if let Some((tool, _)) = tries.first() {
        if cs["tool"] != *tool {
            vec![("crystal_sphere_set_tool".to_string(), json!({"tool": tool}))]
        } else {
            tries.iter().filter(|t| t.0 == *tool).take(6).map(|(_, (x, y))| click(json!(x), json!(y))).collect()
        }
    } else {
        // from the middle: big tools reveal most there
        let size = |v: &Value, default: i64| if v.truthy() { v.int() } else { default };
        let (w, h) = (size(&cs["grid_width"], 9), size(&cs["grid_height"], 11));
        let at = |c: &Value, k: &str| c.get(k).map_or(0.0, |x| x.float());
        let off = |c: &Value| (at(c, "x") - base::floordiv(w, 2) as f64).abs() + (at(c, "y") - base::floordiv(h, 2) as f64).abs();
        let mut cells: Vec<&Value> = cs["clickable_cells"].list().iter().collect();
        base::sort_by_key(&mut cells, |c| off(c));
        cells.iter().take(6).map(|c| click(c["x"].clone(), c["y"].clone())).collect()
    };
    out.push(("crystal_sphere_proceed".to_string(), json!({})));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An 11x11 grid with the given cells already clear.
    fn sphere(clear: &[Cell], items: Value, left: i64) -> Value {
        let cells: Vec<Value> = (0..121).map(|i| (i % 11, i / 11)).map(|(x, y)| json!({"x": x, "y": y, "is_hidden": !clear.contains(&(x, y))})).collect();
        let fogged: Vec<Value> = cells.iter().filter(|c| c["is_hidden"] == true).cloned().collect();
        json!({"crystal_sphere": {"grid_width": 11, "grid_height": 11, "cells": cells, "clickable_cells": fogged, "revealed_items": items,
            "tool": "big", "can_use_big_tool": true, "can_use_small_tool": true, "divinations_left_text": format!("{left} Divinations")}})
    }

    fn clicked(state: &Value) -> (Cell, i64) {
        let (action, params) = &crystal_sphere(state)[0];
        if action == "crystal_sphere_set_tool" {
            let mut with = state.clone();
            with["crystal_sphere"]["tool"] = params["tool"].clone();
            return clicked(&with);
        }
        let reach = (state["crystal_sphere"]["tool"] == "big") as i64;
        ((params["x"].int(), params["y"].int()), reach)
    }

    fn covers(click: (Cell, i64), cell: Cell) -> bool {
        (click.0 .0 - cell.0).abs() <= click.1 && (click.0 .1 - cell.1).abs() <= click.1
    }

    #[test]
    fn an_item_partly_seen_is_finished() {
        // a card reward at 7..9 x 1..3 with its left column clear: one big divination takes the rest
        let item = json!([{"item_type": "CrystalSphereCardReward", "x": 7, "y": 1, "width": 2, "height": 2, "is_good": true}]);
        let state = sphere(&[(7, 1), (7, 2)], item, 1);
        let at = clicked(&state);
        assert!(covers(at, (8, 1)) && covers(at, (8, 2)), "{at:?}");
    }

    #[test]
    fn the_curse_is_never_finished() {
        let curse = json!([{"item_type": "CrystalSphereCurse", "x": 4, "y": 4, "width": 2, "height": 2, "is_good": false}]);
        let state = sphere(&[(4, 4), (4, 5), (5, 4)], curse, 2);
        assert!(!covers(clicked(&state), (5, 5)));
    }

    #[test]
    fn with_nothing_seen_the_most_fog_is_cleared() {
        let at = clicked(&sphere(&[(0, 0), (1, 0), (0, 1)], json!([]), 3));
        assert_eq!(at, ((5, 5), 1));
    }
}
