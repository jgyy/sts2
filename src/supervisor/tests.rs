//! Against the original: tests/golden/supervisor/supervisor.json (each function over made-up inputs, the machine faked on
//! both sides) and sequences.json (whole `Supervisor.run`s in a scripted world: every program run or started, every
//! sleep, every request to the game and every line logged).

use super::testing::*;
use super::*;
use crate::bot::{BotArgs, Stuck};
use crate::client::GameUnavailable;

const GOLDEN: &str = include_str!("../../tests/golden/supervisor/supervisor.json");
const SEQUENCES: &str = include_str!("../../tests/golden/supervisor/sequences.json");

fn golden(part: &str) -> Vec<Value> {
    let all: Value = serde_json::from_str(GOLDEN).unwrap();
    all[part].as_array().unwrap().clone()
}

fn opt(v: &Value) -> Option<String> {
    v.as_str().map(|s| s.to_string())
}

#[test]
fn shlex_as_the_record_has_it() {
    let cases = golden("shlex");
    assert!(cases.len() >= 60);
    for case in cases {
        let s = case["s"].as_str().unwrap();
        assert_eq!(json!(shlex_quote(s)), case["quote"], "{s:?}");
        let split = match shlex_split(s) {
            Ok(words) => json!(words),
            Err(e) => json!({"error": e}),
        };
        assert_eq!(split, case["split"], "{s:?}");
    }
}

#[test]
fn the_game_log_is_read_as_the_record_read_it() {
    for case in golden("mod_load_error") {
        let d = tempfile::tempdir().unwrap();
        let _p = patch(|s| {
            s.GAME_LOG = Some(d.path().join("godot.log"));
            s.set_process_running(|_| false);
        });
        if let Some(text) = case["text"].as_str() {
            std::fs::write(d.path().join("godot.log"), text).unwrap();
        }
        let got = json!({"text": case["text"], "want": mod_load_error(0.0), "future": mod_load_error(4e9), "missing_steam": Sts2Host.missing_steam(0.0)});
        assert_eq!(got, case);
    }
}

#[test]
fn the_game_files_give_the_same_fingerprint() {
    for case in golden("game_fingerprint") {
        let d = tempfile::tempdir().unwrap();
        let game = d.path().join("game");
        let _p = patch(|s| s.environ = Some(env(&[("STS2_GAME_DIR", json!(game.to_string_lossy()))])));
        if let Some(layout) = case["layout"].as_array() {
            std::fs::create_dir(&game).unwrap();
            for row in layout {
                let folder = game.join(row[0].as_str().unwrap());
                std::fs::create_dir(&folder).unwrap();
                std::fs::write(folder.join("sts2.dll"), "x".repeat(row[1].as_u64().unwrap() as usize)).unwrap();
                let ns = row[2].as_u64().unwrap();
                let when = std::time::UNIX_EPOCH + std::time::Duration::new(ns / 1_000_000_000, (ns % 1_000_000_000) as u32);
                File::options().write(true).open(folder.join("sts2.dll")).unwrap().set_modified(when).unwrap();
            }
            std::fs::write(game.join("data_sts2_file"), "not a folder").unwrap();
            std::fs::create_dir(game.join("data_sts2_empty")).unwrap();
        }
        assert_eq!(json!(game_fingerprint()), case["want"], "{}", case["layout"]);
    }
}

#[test]
fn a_stale_mod_is_told_as_recorded() {
    let cases = golden("mod_is_stale");
    assert!(cases.len() >= 42);
    for case in cases {
        let d = tempfile::tempdir().unwrap();
        if case["script"] == true {
            std::fs::write(d.path().join("install.sh"), "#!/bin/sh\n").unwrap();
        }
        if let Some(stamp) = case["stamp"].as_str() {
            std::fs::write(d.path().join("stamp"), stamp).unwrap();
        }
        let fingerprint = opt(&case["fingerprint"]);
        let _p = patch(|s| {
            s.MOD_STAMP = Some(d.path().join("stamp"));
            s.INSTALL_SCRIPT = Some(d.path().join("install.sh"));
            s.set_game_fingerprint(move || fingerprint.clone());
        });
        assert_eq!(json!(mod_is_stale()), case["want"], "{case}");
    }
}

#[test]
fn the_mod_is_rebuilt_as_recorded() {
    let cases = golden("rebuild_mod");
    assert!(cases.len() >= 40);
    for case in cases {
        let d = tempfile::tempdir().unwrap();
        let ran: Rc<RefCell<Vec<Value>>> = Rc::default();
        if case["script"] == true {
            std::fs::write(d.path().join("install.sh"), "#!/bin/sh\n").unwrap();
        }
        let stamp = d.path().join(if case["stamp_dir"] == true { "build" } else { "install.sh" }).join("stamp");
        let (fingerprint, run, seen) = (opt(&case["fingerprint"]), case["run"].clone(), Rc::clone(&ran));
        let _p = patch(|s| {
            s.MOD_STAMP = Some(stamp.clone());
            s.INSTALL_SCRIPT = Some(d.path().join("install.sh"));
            s.set_game_fingerprint(move || fingerprint.clone());
            s.set_run(move |argv, timeout| {
                seen.borrow_mut().push(json!([argv, timeout.map(|t| t as i64)]));
                match run["raise"].as_str() {
                    Some(text) => Err(text.to_string()),
                    None => Ok(Completed { returncode: run["returncode"].as_i64().unwrap(), stdout: run["stdout"].as_str().unwrap().to_string(), stderr: run["stderr"].as_str().unwrap().to_string() }),
                }
            });
        });
        let (want, logs) = logged(rebuild_mod);
        let got = json!({"run": case["run"], "script": case["script"], "fingerprint": case["fingerprint"], "stamp_dir": case["stamp_dir"], "want": want, "ran": *ran.borrow(), "logs": logs,
                         "stamp": std::fs::read_to_string(&stamp).ok()});
        assert_eq!(norm(got, d.path()), case);
    }
}

#[test]
fn a_running_process_is_told_from_a_zombie() {
    for case in golden("process_running") {
        let stdout = case["stdout"].as_str().unwrap().to_string();
        let _p = patch(|s| s.set_run(move |_, _| Ok(Completed { stdout: stdout.clone(), ..Default::default() })));
        assert_eq!(json!(_process_running("x")), case["want"], "{case}");
    }
}

/// The bridge mod as a list of answers (the last one from then on; "unavailable": the game is gone).
struct Mod {
    replies: Vec<Value>,
    calls: Vec<Value>,
}

impl Game for Mod {
    fn state(&mut self) -> Result<Value, GameUnavailable> {
        unreachable!()
    }
    fn act(&mut self, action: &str, params: &Value) -> Result<Value, GameUnavailable> {
        self.calls.push(json!([action, params]));
        let reply = if self.replies.len() > 1 { self.replies.remove(0) } else { self.replies[0].clone() };
        if reply == "unavailable" {
            return Err(GameUnavailable::new("gone"));
        }
        Ok(reply)
    }
    fn profile(&mut self) -> Result<Value, GameUnavailable> {
        unreachable!()
    }
    fn card(&mut self, _card_id: &str) -> Result<Option<Value>, GameUnavailable> {
        unreachable!()
    }
    fn ping(&mut self) -> bool {
        true
    }
}

#[test]
fn the_session_settings_are_sent_as_recorded() {
    let cases = golden("settings");
    assert!(cases.len() >= 29);
    for case in cases {
        let mut game = Mod { replies: case["replies"].as_array().unwrap().clone(), calls: Vec::new() };
        let sleeps: Rc<RefCell<Vec<f64>>> = Rc::default();
        let slept = Rc::clone(&sleeps);
        let _p = patch(|s| {
            s.set_sleep(move |dt| {
                slept.borrow_mut().push(dt);
                Ok(())
            })
        });
        let kw = &case["kw"];
        let (done, logs) = logged(|| match case["fn"].as_str().unwrap() {
            "set_fast_mode" => set_fast_mode(&mut game, kw["mode"].as_str().unwrap()),
            "set_time_scale" => set_time_scale(&mut game, kw["scale"].as_f64().unwrap()),
            _ => {
                let d = SetLightGraphicsArgs::default();
                let args = SetLightGraphicsArgs {
                    width: kw["width"].as_i64().unwrap_or(d.width),
                    height: kw["height"].as_i64().unwrap_or(d.height),
                    maximize: kw["maximize"].as_bool().unwrap_or(d.maximize),
                    fullscreen: kw["fullscreen"].as_bool().unwrap_or(d.fullscreen),
                };
                set_light_graphics(&mut game, args)
            }
        });
        let want = match done {
            Ok(done) => json!(done),
            Err(BotError::GameUnavailable(e)) => json!({"unavailable": e.to_string()}),
            Err(e) => panic!("{e}"),
        };
        let failed: Vec<bool> = case["replies"].as_array().unwrap().iter().filter(|r| r.is_object()).map(_failed).collect();
        let got = json!({"fn": case["fn"], "kw": kw, "replies": case["replies"], "want": want, "calls": game.calls, "sleeps": *sleeps.borrow(), "logs": logs, "failed": failed});
        assert_eq!(got, case);
    }
}

#[test]
fn a_scope_of_its_own_under_systemd_as_recorded() {
    let cases = golden("detached");
    assert!(cases.len() >= 27);
    for case in cases {
        let which = opt(&case["which"]);
        let _p = patch(|s| {
            s.environ = Some(env(&[("INVOCATION_ID", case["invocation"].clone())]));
            s.set_which(move |_| which.clone());
        });
        assert_eq!(json!(_detached(&words(&case["argv"]))), case["want"], "{case}");
    }
}

#[test]
fn the_game_is_found_as_recorded() {
    use std::os::unix::fs::PermissionsExt;
    let cases = golden("default_launch");
    assert!(cases.len() >= 14);
    for case in cases {
        let d = tempfile::tempdir().unwrap();
        if !case["game_dir"].is_null() {
            let _p = patch(|s| {
                let mut e = env(&[("STS2_GAME_DIR", case["folder"].clone())]);
                e.insert("HOME".to_string(), "/home/someone".to_string());
                s.environ = Some(e);
            });
            assert_eq!(json!({"folder": case["folder"], "exe": null, "want": default_launch(), "game_dir": game_dir().to_string_lossy()}), case);
            continue;
        }
        let folder = d.path().join(case["folder"].as_str().unwrap());
        let _p = patch(|s| s.environ = Some(env(&[("STS2_GAME_DIR", json!(folder.to_string_lossy()))])));
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("SlayTheSpire2");
        match case["exe"].as_str() {
            Some("dir") => std::fs::create_dir(&path).unwrap(),
            Some(kind) => {
                std::fs::write(&path, "x").unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(if kind == "exe" { 0o755 } else { 0o644 })).unwrap();
            }
            None => {}
        }
        assert_eq!(norm(json!({"folder": case["folder"], "exe": case["exe"], "want": default_launch()}), d.path()), case);
    }
}

/// A small world of processes (the original's `World` in the script that made the golden file).
struct World {
    sc: Value,
    t: f64,
    trace: Vec<Value>,
    steam_at: Option<f64>,
    game: Value,
    term_at: Option<f64>,
}

impl World {
    fn new(sc: &Value) -> Rc<RefCell<World>> {
        Rc::new(RefCell::new(World { sc: sc.clone(), t: 0.0, trace: Vec::new(), steam_at: if sc["steam"] == true { Some(0.0) } else { None }, game: sc["game"].clone(), term_at: None }))
    }

    fn alive(&self, name: &str) -> bool {
        match name {
            "steam" => self.steam_at.is_some(),
            "steamwebhelper" => match (self.steam_at, self.sc["webhelper_after"].as_f64()) {
                (Some(at), Some(after)) => self.t >= at + after,
                _ => false,
            },
            "SlayTheSpire2" => {
                let died = self.term_at.is_some_and(|at| self.game["ignores_term"] != true && self.t >= at + self.game["dies_after"].as_f64().unwrap_or(0.0));
                !self.game.is_null() && self.game["dead"] != true && !died
            }
            _ => false,
        }
    }

    fn patch(world: &Rc<RefCell<World>>) -> Patched {
        patch(|s| {
            let w = Rc::clone(world);
            s.set_run(move |argv, _| {
                let mut w = w.borrow_mut();
                w.trace.push(json!(["run", argv]));
                let mut stdout = "";
                if argv[0] == "ps" {
                    stdout = if w.alive(&argv[2]) { "Sl\n" } else { "" };
                } else if argv[0] == "pkill" {
                    if argv.iter().any(|a| a == "-9") && !w.game.is_null() {
                        w.game["dead"] = json!(true);
                    } else if w.term_at.is_none() {
                        w.term_at = Some(w.t);
                    }
                }
                Ok(Completed { stdout: stdout.to_string(), ..Default::default() })
            });
            let w = Rc::clone(world);
            s.set_popen(move |argv, kw| {
                let mut w = w.borrow_mut();
                let shown = kw.env.as_ref().map(|e| {
                    let mut some = serde_json::Map::new();
                    for k in ["SteamAppId", "SteamGameId", "STS2_DEV_SKIP", "KEPT"] {
                        some.insert(k.to_string(), json!(e.get(k)));
                    }
                    some
                });
                // (always a session of its own and no input; no output kept unless a file is given)
                w.trace.push(json!(["popen", argv, kw.cwd.as_ref().map(|c| c.to_string_lossy()), shown, true, [true, kw.out.is_none(), kw.out.is_none()]]));
                let program = if argv[0] == "systemd-run" { &argv[argv.iter().position(|a| a == "--").unwrap() + 1] } else { &argv[0] };
                let name = Path::new(program).file_name().unwrap().to_string_lossy().into_owned();
                if w.sc["fails"].as_array().is_some_and(|f| f.contains(&json!(name))) {
                    return Err(std::io::Error::from_raw_os_error(2));
                }
                if name == "steam" {
                    w.steam_at = Some(w.t);
                }
                Ok(Box::new(Running))
            });
            let w = Rc::clone(world);
            s.set_which(move |name| w.borrow().sc["which"][name].as_str().map(|p| p.to_string()));
            let w = Rc::clone(world);
            s.set_sleep(move |dt| {
                let mut w = w.borrow_mut();
                w.trace.push(json!(["sleep", dt]));
                w.t += dt;
                Ok(())
            });
            let w = Rc::clone(world);
            s.set_monotonic(move || w.borrow().t);
            let mut e = env(&[("KEPT", json!("yes"))]);
            for (k, v) in world.borrow().sc["env"].as_object().into_iter().flatten() {
                e.insert(k.clone(), v.as_str().unwrap().to_string());
            }
            s.environ = Some(e);
        })
    }
}

#[test]
fn the_game_is_launched_as_recorded() {
    let cases = golden("launch_game");
    assert!(cases.len() >= 14);
    for case in cases {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("game dir")).unwrap();
        let world = World::new(&case["sc"]);
        let _p = World::patch(&world);
        _children(|c| c.clear());
        let cmd = case["sc"]["cmd"].as_str().unwrap().replace("{T}", &d.path().to_string_lossy());
        let (_, logs) = logged(|| launch_game(&cmd).unwrap());
        let children = _children(|c| c.len());
        _children(|c| c.clear());
        let got = json!({"sc": case["sc"], "trace": world.borrow().trace, "logs": logs, "children": children});
        assert_eq!(norm(got, d.path()), case);
    }
}

#[test]
fn the_game_is_killed_as_recorded() {
    let cases = golden("kill_game");
    assert!(cases.len() >= 9);
    for case in cases {
        let world = World::new(&case["sc"]);
        let _p = World::patch(&world);
        kill_game(case["sc"]["grace"].as_f64().unwrap_or(10.0)).unwrap();
        let got = json!({"sc": case["sc"], "trace": world.borrow().trace, "running": world.borrow().alive("SlayTheSpire2")});
        assert_eq!(got, case);
    }
}

/// The scripted world of sequences.json (the original's `World` and `Bot` in the script that made it): time that moves
/// only in sleeps; Steam and the game as processes; the game's API as its boot says; the bot as its script says.
struct Scripted {
    sc: Value,
    tmp: PathBuf,
    t: f64,
    trace: Vec<Value>,
    steam_at: Option<f64>,
    /// (how it boots, when it was started, killed)
    game: Option<(Value, f64, bool)>,
    boots: Vec<Value>,
    rebuilds: Vec<Value>,
    script: Vec<Value>,
    replies: serde_json::Map<String, Value>,
    down_until: f64,
    fingerprint: Option<String>,
    asked: i64,
}

/// The first of the list while there is more than one, then the last one for ever; None for an empty list.
fn next(list: &mut Vec<Value>) -> Option<Value> {
    match list.len() {
        0 => None,
        1 => Some(list[0].clone()),
        _ => Some(list.remove(0)),
    }
}

impl Scripted {
    fn list(v: &Value) -> Vec<Value> {
        v.as_array().cloned().unwrap_or_default()
    }

    fn new(sc: &Value, tmp: &Path) -> Rc<RefCell<Scripted>> {
        let mut world = Scripted {
            sc: sc.clone(),
            tmp: tmp.to_path_buf(),
            t: 0.0,
            trace: Vec::new(),
            steam_at: if sc["steam"] == true { Some(0.0) } else { None },
            game: None,
            boots: Self::list(&sc["boots"]),
            rebuilds: Self::list(&sc["rebuilds"]),
            script: Self::list(&sc["bot"]),
            replies: sc["replies"].as_object().cloned().unwrap_or_default(),
            down_until: 0.0,
            fingerprint: opt(&sc["fingerprint"]),
            asked: 0,
        };
        if sc["external"] == true {
            world.start_game();
        }
        Rc::new(RefCell::new(world))
    }

    fn start_game(&mut self) {
        let spec = next(&mut self.boots).unwrap_or(json!({"api_after": 5.0}));
        let game_log = self.tmp.join("godot.log");
        match spec["log"].as_str() {
            None => {
                let _ = std::fs::remove_file(&game_log);
            }
            Some(text) => std::fs::write(&game_log, text).unwrap(),
        }
        self.game = Some((spec, self.t, false));
    }

    fn alive(&self, name: &str) -> bool {
        match name {
            "steam" => self.steam_at.is_some() || self.sc["steam_appears"].as_f64().is_some_and(|at| self.t >= at),
            "steamwebhelper" => self.steam_at.is_some_and(|at| self.t >= at + self.sc["webhelper_after"].as_f64().unwrap_or(0.0)),
            "SlayTheSpire2" => match &self.game {
                Some((spec, at, killed)) => !killed && spec["dies_after"].as_f64().is_none_or(|after| self.t < at + after),
                None => false,
            },
            _ => false,
        }
    }

    fn api(&self) -> bool {
        match &self.game {
            Some((spec, at, _)) => self.alive("SlayTheSpire2") && spec["api_after"].as_f64().is_some_and(|after| self.t >= at + after) && self.t >= self.down_until,
            None => false,
        }
    }

    fn note(&mut self, entry: Value) {
        self.trace.push(entry);
        assert!(self.trace.len() < 5000, "the scenario does not end");
    }

    fn patch(world: &Rc<RefCell<Scripted>>, script: &Path, stamp: &Path) -> Patched {
        patch(|s| {
            let w = Rc::clone(world);
            s.set_run(move |argv, _| {
                let mut w = w.borrow_mut();
                w.note(json!(["run", argv]));
                match argv[0].as_str() {
                    "ps" => Ok(Completed { stdout: if w.alive(&argv[2]) { "Sl\n" } else { "" }.to_string(), ..Default::default() }),
                    "pkill" => {
                        let hard = argv.iter().any(|a| a == "-9");
                        if let Some((spec, _, killed)) = w.game.as_mut() {
                            if hard || spec["ignores_term"] != true {
                                *killed = true;
                            }
                        }
                        Ok(Completed::default())
                    }
                    "bash" => {
                        let r = next(&mut w.rebuilds).unwrap_or(json!({"returncode": 0}));
                        if let Some(text) = r["raise"].as_str() {
                            return Err(text.to_string());
                        }
                        if let Some(fingerprint) = r["fingerprint"].as_str() {
                            w.fingerprint = Some(fingerprint.to_string());
                        }
                        Ok(Completed { returncode: r["returncode"].as_i64().unwrap(), stdout: r["stdout"].as_str().unwrap_or("").to_string(), stderr: r["stderr"].as_str().unwrap_or("").to_string() })
                    }
                    other => panic!("{other}"),
                }
            });
            let w = Rc::clone(world);
            s.set_popen(move |argv, kw| {
                let mut w = w.borrow_mut();
                let shown = kw.env.as_ref().map(|e| {
                    let mut some = serde_json::Map::new();
                    for k in ["SteamAppId", "SteamGameId", "INVOCATION_ID"] {
                        some.insert(k.to_string(), json!(e.get(k)));
                    }
                    some
                });
                w.note(json!(["popen", argv, kw.cwd.as_ref().map(|c| c.to_string_lossy()), shown]));
                let program = if argv[0] == "systemd-run" { &argv[argv.iter().position(|a| a == "--").unwrap() + 1] } else { &argv[0] };
                let name = Path::new(program).file_name().unwrap().to_string_lossy().into_owned();
                if name == "steam" {
                    w.steam_at = Some(w.t);
                }
                if name == "SlayTheSpire2" || argv.iter().any(|a| a == "-applaunch") {
                    w.start_game();
                }
                Ok(Box::new(Running))
            });
            let w = Rc::clone(world);
            s.set_which(move |name| w.borrow().sc["which"][name].as_str().map(|p| p.to_string()));
            let w = Rc::clone(world);
            s.set_sleep(move |dt| {
                let mut w = w.borrow_mut();
                w.note(json!(["sleep", dt]));
                w.t += dt;
                Ok(())
            });
            let w = Rc::clone(world);
            s.set_monotonic(move || w.borrow().t);
            let w = Rc::clone(world);
            s.set_time(move || 1000.0 + w.borrow().t);
            let w = Rc::clone(world);
            s.set_game_fingerprint(move || w.borrow().fingerprint.clone());
            let w = Rc::clone(world);
            s.set_bot_run(move |bot, max_runs| {
                let mut w = w.borrow_mut();
                let careful_at = bot.careful_at.as_ref().map(|at| json!([at.0, at.1.unwrap().0, at.1.unwrap().1]));
                w.note(json!(["bot.run", max_runs, bot.abandon_next, bot.time_scale, bot.careful_until, careful_at]));
                let step = if w.script.is_empty() { json!({"runs": 1}) } else { w.script.remove(0) };
                bot.abandon_next = false;
                bot.runs_finished += step["runs"].as_i64().unwrap_or(0);
                if let Some(at) = step["where"].as_array() {
                    bot._last_where = Some((at[0].as_i64(), at[1].as_i64()));
                }
                // (asked something that may have crashed the game: `note_crash` then says there is another way, once)
                w.asked += 1;
                bot._last_sent = step["sent"].truthy().then(|| (json!(0), json!(0), "play".to_string(), w.asked.to_string()));
                if step["kill"] == true {
                    w.game.as_mut().unwrap().2 = true;
                }
                if let Some(down) = step["down"].as_f64() {
                    w.down_until = w.t + down;
                }
                match step["raise"].as_str() {
                    Some("unavailable") => Err(BotError::GameUnavailable(GameUnavailable::new("Connection refused"))),
                    Some("stuck") => Err(BotError::Stuck(Stuck(format!("no progress for 300s at {}", base::repr_of(&step["where"]))))),
                    _ => Ok(()),
                }
            });
            let mut e = env(&[]);
            for (k, v) in world.borrow().sc["env"].as_object().into_iter().flatten() {
                e.insert(k.clone(), v.as_str().unwrap().to_string());
            }
            s.environ = Some(e);
            s.GAME_LOG = Some(world.borrow().tmp.join("godot.log"));
            s.MOD_STAMP = Some(stamp.to_path_buf());
            s.INSTALL_SCRIPT = Some(script.to_path_buf());
        })
    }
}

/// The game as the supervisor and the settings see it.
struct Api(Rc<RefCell<Scripted>>);

impl Game for Api {
    fn state(&mut self) -> Result<Value, GameUnavailable> {
        unreachable!()
    }
    fn act(&mut self, action: &str, params: &Value) -> Result<Value, GameUnavailable> {
        let mut w = self.0.borrow_mut();
        w.note(json!(["act", action, params]));
        let reply = match w.replies.get_mut(action).and_then(|r| r.as_array_mut()).filter(|r| !r.is_empty()) {
            Some(replies) => next(replies).unwrap(),
            None => json!({"status": "ok", "fullscreen": true, "maximized": true, "fps_before": 60}),
        };
        if reply == "unavailable" {
            return Err(GameUnavailable::new("the mod is gone"));
        }
        Ok(reply)
    }
    fn profile(&mut self) -> Result<Value, GameUnavailable> {
        unreachable!()
    }
    fn card(&mut self, _card_id: &str) -> Result<Option<Value>, GameUnavailable> {
        unreachable!()
    }
    fn ping(&mut self) -> bool {
        self.0.borrow().api()
    }
}

#[test]
fn the_supervisor_does_as_the_record_did_in_a_scripted_world() {
    let cases: Vec<Value> = serde_json::from_str(SEQUENCES).unwrap();
    assert!(cases.len() >= 26);
    for case in cases {
        let sc = &case["sc"];
        let d = tempfile::tempdir().unwrap();
        let world = Scripted::new(sc, d.path());
        let script = d.path().join("install_mod.sh");
        if sc["script"] != false {
            std::fs::write(&script, "#!/bin/sh\n").unwrap();
        }
        let stamp = d.path().join("build").join("mod_built_for");
        if let Some(text) = sc["stamp"].as_str() {
            std::fs::create_dir(stamp.parent().unwrap()).unwrap();
            std::fs::write(&stamp, text).unwrap();
        }
        let _p = Scripted::patch(&world, &script, &stamp);
        _children(|c| c.clear());
        let clock = Rc::clone(&world);
        // (try_borrow: the bot's clock is read while the scripted bot holds the world)
        let mut bot = Bot::new(Rc::new(RefCell::new(Api(Rc::clone(&world)))), BotArgs { clock: Some(Box::new(move || clock.try_borrow().map(|w| w.t).unwrap_or(f64::NAN))), sleep: Some(Box::new(|_| {})), ..Default::default() });
        bot._last_where = Some((Some(1), Some(1)));
        let a = &sc["args"];
        let d_args = SupervisorArgs::default();
        let args = SupervisorArgs {
            host: None,
            launch_cmd: sc["cmd"].as_str().unwrap_or("/games/sts2/SlayTheSpire2").to_string(),
            manage_game: a["manage_game"].as_bool().unwrap_or(d_args.manage_game),
            fast_mode: if a.has("fast_mode") { opt(&a["fast_mode"]) } else { d_args.fast_mode.clone() },
            light_graphics: a["light_graphics"].as_bool().unwrap_or(d_args.light_graphics),
            small_window: a["small_window"].as_bool().unwrap_or(d_args.small_window),
            fullscreen: a["fullscreen"].as_bool().unwrap_or(d_args.fullscreen),
            time_scale: a["time_scale"].as_f64().unwrap_or(d_args.time_scale),
            boot_timeout: a["boot_timeout"].as_f64().unwrap_or(d_args.boot_timeout),
            outage_grace: a["outage_grace"].as_f64().unwrap_or(d_args.outage_grace),
            heal_mod: a["heal_mod"].as_bool().unwrap_or(d_args.heal_mod),
        };
        let mut sup = Supervisor::new(bot, Rc::new(RefCell::new(Api(Rc::clone(&world)))), args);
        let (ended, logs) = logged(|| sup.run(Some(sc["max_runs"].as_i64().unwrap_or(1))));
        ended.unwrap();
        let careful_at = sup.bot.careful_at.as_ref().map(|at| json!([at.0, at.1.unwrap().0, at.1.unwrap().1]));
        let end = json!({"restarts": sup.restarts, "runs": sup.bot.runs_finished, "abandon_next": sup.bot.abandon_next, "careful_until": sup.bot.careful_until, "careful_at": careful_at,
                         "time_scale": sup.bot.time_scale, "mod_retry_at": sup._mod_retry_at, "mod_backoff": sup._mod_backoff, "crash_repeats": sup._crash_repeats, "heal_mod": sup.heal_mod,
                         "t": world.borrow().t, "stamp": std::fs::read_to_string(&stamp).ok(), "children": _children(|c| c.len())});
        _children(|c| c.clear());
        let got = norm(json!({"sc": sc, "trace": world.borrow().trace, "logs": logs, "end": end}), d.path());
        for part in ["trace", "logs", "end"] {
            assert_eq!(got[part], case[part], "{}: {}", sc["name"], part);
        }
    }
}
