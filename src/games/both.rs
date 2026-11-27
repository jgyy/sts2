//! `sts2bot --game both`: a bot process for each game (`--game sts2`, `--game sts1`, other options passed on), kept going
//! as the systemd unit would: one that fails is restarted after a pause, one that finished its --runs stays finished, and Ctrl+C or
//! SIGTERM stops both. (Processes, not threads: the game is a switch for a whole bot, and two processes plan on two cores.)

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use super::GAMES;
use crate::logging::Logger;
use crate::{log_info, log_warning};

#[allow(non_upper_case_globals)]
pub static log: Logger = Logger::new("sts2bot.both");

crate::tunables! {
    pub RESTART_DELAY: f64 = 15.0; // as the unit's RestartSec
    pub MAX_DELAY: f64 = 300.0; // a bot that keeps failing straight away is tried less and less often, down to this
}
pub const HEALTHY: f64 = 600.0; // a bot that ran this long before it failed is tried again after the short pause
pub const RELOAD_DELAY: f64 = 1.0; // a bot ended by SIGUSR1 (a deliberate reload) comes back after this, whatever its back-off (2026-11-26: five reloads in an hour had it at 240 s)
pub const STOP_GRACE: f64 = 10.0;
pub const GAME_OPTIONS: [&str; 3] = ["--port", "--log-dir", "--launch-cmd"]; // each game needs its own value: not allowed with both

/// Where the bots are started (the directory the program was built in, as Python's started where `sts2bot` was imported from);
/// None if it is not there: then where we are.
pub fn ROOT() -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.is_dir().then_some(root)
}

/// Ctrl+C, or SIGTERM from systemd (Python's KeyboardInterrupt).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyboardInterrupt;

/// A bot process as `run` sees it (Python's `subprocess.Popen`; tests put their own in its place).
pub trait Proc {
    fn pid(&self) -> i64;
    /// None while it runs; then its exit code (minus the signal that ended it).
    fn poll(&mut self) -> Option<i64>;
    fn send_signal(&mut self, sig: i32);
    fn kill(&mut self);
    fn wait(&mut self) -> Option<i64>;
}

fn exit_code(status: std::process::ExitStatus) -> i64 {
    use std::os::unix::process::ExitStatusExt;
    status.code().map(|c| c as i64).unwrap_or_else(|| -(status.signal().unwrap_or(0) as i64))
}

/// A real process.
pub struct Popen {
    pub child: std::process::Child,
    pub returncode: Option<i64>,
}

impl Proc for Popen {
    fn pid(&self) -> i64 {
        self.child.id() as i64
    }

    fn poll(&mut self) -> Option<i64> {
        if self.returncode.is_none() {
            if let Ok(Some(status)) = self.child.try_wait() {
                self.returncode = Some(exit_code(status));
            }
        }
        self.returncode
    }

    fn send_signal(&mut self, sig: i32) {
        if self.poll().is_none() {
            unsafe { libc::kill(self.child.id() as libc::pid_t, sig) };
        }
    }

    fn kill(&mut self) {
        self.send_signal(libc::SIGKILL);
    }

    fn wait(&mut self) -> Option<i64> {
        if self.returncode.is_none() {
            if let Ok(status) = self.child.wait() {
                self.returncode = Some(exit_code(status));
            }
        }
        self.returncode
    }
}

/// `subprocess.Popen(argv, cwd=ROOT, stdin=subprocess.DEVNULL)`.
pub fn popen(argv: &[String]) -> Box<dyn Proc> {
    let mut cmd = std::process::Command::new(&argv[0]);
    cmd.args(&argv[1..]).stdin(std::process::Stdio::null());
    if let Some(root) = ROOT() {
        cmd.current_dir(root);
    }
    let child = cmd.spawn().unwrap_or_else(|e| panic!("could not start {}: {}", argv[0], e));
    Box::new(Popen { child, returncode: None })
}

// The signals: one handler for the whole process (a signal is the process's), which only notes that we were told to stop.
static STOPPED: AtomicBool = AtomicBool::new(false);
static HANDLING: Mutex<(usize, libc::sighandler_t, libc::sighandler_t)> = Mutex::new((0, 0, 0));

extern "C" fn stop(_signum: libc::c_int) {
    STOPPED.store(true, Ordering::SeqCst);
}

/// True once SIGTERM or Ctrl+C came while a `run` was going.
pub fn stopped() -> bool {
    STOPPED.load(Ordering::SeqCst)
}

/// SIGTERM and Ctrl+C note that we are to stop, until dropped (then they do what they did before).
struct Handling;

impl Handling {
    fn new() -> Self {
        let mut h = HANDLING.lock().unwrap_or_else(|e| e.into_inner());
        if h.0 == 0 {
            STOPPED.store(false, Ordering::SeqCst);
            let handler = stop as extern "C" fn(libc::c_int) as libc::sighandler_t;
            h.1 = unsafe { libc::signal(libc::SIGTERM, handler) };
            h.2 = unsafe { libc::signal(libc::SIGINT, handler) };
        }
        h.0 += 1;
        Handling
    }
}

impl Drop for Handling {
    fn drop(&mut self) {
        let mut h = HANDLING.lock().unwrap_or_else(|e| e.into_inner());
        h.0 -= 1;
        if h.0 == 0 {
            unsafe {
                libc::signal(libc::SIGTERM, h.1);
                libc::signal(libc::SIGINT, h.2);
            }
        }
    }
}

/// `time.monotonic()`.
pub fn monotonic() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}

/// `time.sleep(seconds)`, left early (with the interrupt) when we are told to stop.
pub fn sleep(seconds: f64) -> Result<(), KeyboardInterrupt> {
    let until = monotonic() + seconds;
    loop {
        if stopped() {
            return Err(KeyboardInterrupt);
        }
        let left = until - monotonic();
        if left <= 0.0 {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_secs_f64(left.min(0.05)));
    }
}

/// The command line with its --game option taken out (`--game both`, `--game=both`, `--ga both` ...).
pub fn without_game(argv: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut skip = false;
    for word in argv {
        if skip {
            skip = false;
            continue;
        }
        let name = word.split('=').next().unwrap_or("");
        if name.chars().count() > 2 && "--game".starts_with(name) {
            skip = !word.contains('=');
            continue;
        }
        out.push(word.clone());
    }
    out
}

/// (Python's `[sys.executable, "-m", "sts2bot", "--game", game, *rest]`: here the program itself stands for the first three.)
pub fn command(game: &str, rest: &[String]) -> Vec<String> {
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "sts2bot".to_string());
    let mut out = vec![exe, "--game".to_string(), game.to_string()];
    out.extend(rest.iter().cloned());
    out
}

/// `run`'s keyword arguments: the games to play, and what starts a bot, tells the time and waits (tests bring their own;
/// a `sleep` that gives the interrupt stands for Ctrl+C or SIGTERM arriving meanwhile).
pub struct RunArgs<'a> {
    pub games: Vec<String>,
    pub spawn: Box<dyn FnMut(&[String]) -> Box<dyn Proc> + 'a>,
    pub clock: Box<dyn FnMut() -> f64 + 'a>,
    pub sleep: Box<dyn FnMut(f64) -> Result<(), KeyboardInterrupt> + 'a>,
}

impl Default for RunArgs<'_> {
    fn default() -> Self {
        RunArgs { games: GAMES.iter().map(|g| g.to_string()).collect(), spawn: Box::new(popen), clock: Box::new(monotonic), sleep: Box::new(sleep) }
    }
}

/// Play every game in `games` at once until each bot finishes (or forever, without --runs). Returns 0.
pub fn run(argv: &[String], args: RunArgs) -> i32 {
    let RunArgs { games, mut spawn, mut clock, mut sleep } = args;
    let rest = without_game(argv);
    let mut procs: Vec<(String, Box<dyn Proc>)> = Vec::new(); // (in the order they were started, as Python's dict)
    let mut started: HashMap<String, f64> = HashMap::new();
    let mut due: HashMap<String, f64> = games.iter().map(|g| (g.clone(), 0.0)).collect(); // when each bot is to be (re)started
    let mut delay: HashMap<String, f64> = games.iter().map(|g| (g.clone(), RESTART_DELAY.get())).collect();
    let mut done: HashSet<String> = HashSet::new();

    let handling = Handling::new();
    let mut body = || -> Result<(), KeyboardInterrupt> {
        while done.len() < games.len() {
            let now = clock();
            for game in &games {
                if done.contains(game) {
                    continue;
                }
                let at = procs.iter().position(|(g, _)| g == game);
                let Some(at) = at else {
                    if now >= due[game] {
                        let proc = spawn(&command(game, &rest));
                        started.insert(game.clone(), now);
                        log_info!(log, "started the {} bot (pid {})", game, proc.pid());
                        procs.push((game.clone(), proc));
                    }
                    continue;
                };
                let Some(code) = procs[at].1.poll() else {
                    continue;
                };
                procs.remove(at);
                if code == 0 {
                    done.insert(game.clone());
                    log_info!(log, "the {} bot finished", game);
                    continue;
                }
                if code == -(libc::SIGUSR1 as i64) {
                    // a reload asked for (`kill -USR1 <bot>`, after a code change): back at once, no back-off
                    due.insert(game.clone(), now + RELOAD_DELAY);
                    log_info!(log, "the {} bot was told to reload; starting it again", game);
                    continue;
                }
                if now - started[game] >= HEALTHY {
                    delay.insert(game.clone(), RESTART_DELAY.get());
                }
                due.insert(game.clone(), now + delay[game]);
                log_warning!(log, "the {} bot stopped (exit code {}); starting it again in {:.0} s", game, code, delay[game]);
                delay.insert(game.clone(), (delay[game] * 2.0).min(MAX_DELAY.get()));
            }
            if stopped() {
                return Err(KeyboardInterrupt);
            }
            sleep(1.0)?;
            if stopped() {
                return Err(KeyboardInterrupt);
            }
        }
        Ok(())
    };
    let _ = body();
    drop(handling);
    let mut left: Vec<Box<dyn Proc>> = procs.into_iter().map(|(_, p)| p).collect();
    match _stop(&mut left, &mut *clock, &mut *sleep) {
        Ok(()) => 0,
        Err(KeyboardInterrupt) => 130, // (stopped again while stopping: Python left with the interrupt, the bots as they were)
    }
}

pub fn _wait(procs: &mut [Box<dyn Proc>], seconds: f64, clock: &mut dyn FnMut() -> f64, sleep: &mut dyn FnMut(f64) -> Result<(), KeyboardInterrupt>) -> Result<(), KeyboardInterrupt> {
    let deadline = clock() + seconds;
    while clock() < deadline && procs.iter_mut().any(|p| p.poll().is_none()) {
        sleep(0.2)?;
    }
    Ok(())
}

/// Stop the bots still running (the games stay up): a moment to stop by themselves (Ctrl+C and SIGTERM reach them too), then
/// a Ctrl+C of their own, then a kill.
pub fn _stop(procs: &mut [Box<dyn Proc>], clock: &mut dyn FnMut() -> f64, sleep: &mut dyn FnMut(f64) -> Result<(), KeyboardInterrupt>) -> Result<(), KeyboardInterrupt> {
    _wait(procs, STOP_GRACE / 4.0, clock, sleep)?;
    for proc in procs.iter_mut() {
        if proc.poll().is_none() {
            proc.send_signal(libc::SIGINT);
        }
    }
    _wait(procs, STOP_GRACE, clock, sleep)?;
    for proc in procs.iter_mut() {
        if proc.poll().is_none() {
            proc.kill();
            proc.wait();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn words(w: &[&str]) -> Vec<String> {
        w.iter().map(|x| x.to_string()).collect()
    }

    #[derive(Default)]
    struct Fake {
        argv: Vec<String>,
        pid: i64,
        code: Option<i64>,
        signals: Vec<i32>,
    }

    struct FakeProc(Rc<RefCell<Fake>>);

    impl Proc for FakeProc {
        fn pid(&self) -> i64 {
            self.0.borrow().pid
        }
        fn poll(&mut self) -> Option<i64> {
            self.0.borrow().code
        }
        fn send_signal(&mut self, sig: i32) {
            let mut f = self.0.borrow_mut();
            f.signals.push(sig);
            f.code = Some(0); // a bot stops on Ctrl+C and says how many runs it played
        }
        fn kill(&mut self) {
            self.0.borrow_mut().code = Some(-9);
        }
        fn wait(&mut self) -> Option<i64> {
            self.0.borrow().code
        }
    }

    /// A clock that moves only when `run` sleeps, and bots that exit when `script` says (those started from `until` on: at once).
    #[derive(Default)]
    struct Rig {
        t: f64,
        procs: Vec<Rc<RefCell<Fake>>>,
        until: f64,
    }

    impl Rig {
        fn of(&self, game: &str) -> Vec<Rc<RefCell<Fake>>> {
            self.procs.iter().filter(|p| p.borrow().argv[2] == game).cloned().collect()
        }
    }

    fn play(rig: &Rc<RefCell<Rig>>, argv: &[&str], games: &[&str], mut script: impl FnMut(&mut Rig) -> Result<(), KeyboardInterrupt>) -> i32 {
        let (a, b, c) = (rig.clone(), rig.clone(), rig.clone());
        let args = RunArgs {
            games: words(games),
            spawn: Box::new(move |argv| {
                let mut r = a.borrow_mut();
                let fake = Rc::new(RefCell::new(Fake { argv: argv.to_vec(), pid: 1000 + r.procs.len() as i64, ..Default::default() }));
                if r.t >= r.until {
                    fake.borrow_mut().code = Some(0);
                }
                r.procs.push(fake.clone());
                Box::new(FakeProc(fake))
            }),
            clock: Box::new(move || b.borrow().t),
            sleep: Box::new(move |dt| {
                let mut r = c.borrow_mut();
                r.t += dt;
                script(&mut r)
            }),
        };
        crate::logging::capture(|| run(&words(argv), args)).0
    }

    fn rig(until: f64) -> Rc<RefCell<Rig>> {
        Rc::new(RefCell::new(Rig { until, ..Default::default() }))
    }

    #[test]
    fn the_game_option_is_taken_out_and_the_rest_passed_on() {
        for argv in [&["--game", "both", "-v"][..], &["--game=both", "-v"], &["-v", "--ga", "both"]] {
            assert_eq!(without_game(&words(argv)), words(&["-v"]));
        }
        assert_eq!(without_game(&words(&["--g", "x", "-g", "--gamex", "--", "--game"])), words(&["-g", "--gamex", "--"]));
        assert_eq!(command("sts1", &words(&["--runs", "3"]))[1..], words(&["--game", "sts1", "--runs", "3"]));
    }

    #[test]
    fn each_game_gets_a_bot_and_one_that_fails_is_started_again() {
        let r = rig(f64::INFINITY);
        let code = play(&r, &["--game", "both", "--runs", "2", "-v"], &["sts2", "sts1"], |rig| {
            let (sts2, sts1) = (rig.of("sts2"), rig.of("sts1"));
            if rig.t >= 5.0 && sts1.len() == 1 {
                sts1[0].borrow_mut().code = Some(1); // crashed
            }
            if rig.t >= 40.0 && sts1.len() == 2 {
                sts1[1].borrow_mut().code = Some(0); // played its runs
            }
            if rig.t >= 50.0 {
                sts2[0].borrow_mut().code = Some(0);
            }
            Ok(())
        });
        assert_eq!(code, 0);
        let r = r.borrow();
        assert_eq!((r.of("sts2").len(), r.of("sts1").len()), (1, 2));
        assert_eq!(r.procs[0].borrow().argv[1..], words(&["--game", "sts2", "--runs", "2", "-v"]));
        assert!(r.procs.iter().all(|p| p.borrow().signals.is_empty())); // every one stopped by itself
    }

    #[test]
    fn a_bot_that_keeps_failing_is_tried_less_often() {
        let starts: Rc<RefCell<Vec<f64>>> = Rc::default();
        let seen = starts.clone();
        let r = rig(200.0);
        let (_a, _b) = (RESTART_DELAY.set(10.0), MAX_DELAY.set(40.0));
        play(&r, &["--game", "both"], &["sts1"], move |rig| {
            for p in rig.of("sts1") {
                if p.borrow().code.is_none() {
                    // fails as soon as it starts
                    seen.borrow_mut().push(rig.t);
                    p.borrow_mut().code = Some(2);
                }
            }
            Ok(())
        });
        let starts = starts.borrow();
        let gaps: Vec<i64> = starts.windows(2).map(|w| crate::py::round(w[1] - w[0])).collect();
        assert_eq!(gaps[..4], [11, 21, 41, 41]); // (1 s of polling on top of each pause)
    }

    #[test]
    fn a_bot_told_to_reload_is_back_at_once_whatever_its_back_off() {
        let starts: Rc<RefCell<Vec<f64>>> = Rc::default();
        let seen = starts.clone();
        let r = rig(30.0);
        play(&r, &["--game", "both"], &["sts1"], move |rig| {
            for p in rig.of("sts1") {
                if p.borrow().code.is_none() {
                    // reloaded (SIGUSR1) as soon as it starts
                    seen.borrow_mut().push(rig.t);
                    p.borrow_mut().code = Some(-(libc::SIGUSR1 as i64));
                }
            }
            Ok(())
        });
        let starts = starts.borrow();
        assert!(starts.len() > 1 && starts.windows(2).all(|w| w[1] - w[0] <= 3.0)); // (no doubling: a second or two each time)
    }

    #[test]
    fn stopping_stops_every_bot() {
        let r = rig(f64::INFINITY);
        let mut stopped = false;
        let code = play(&r, &["--game", "both"], &["sts2", "sts1"], move |rig| {
            if rig.t >= 10.0 && !stopped {
                stopped = true;
                return Err(KeyboardInterrupt); // Ctrl+C, or SIGTERM from systemd
            }
            Ok(())
        });
        assert_eq!(code, 0);
        let r = r.borrow();
        assert_eq!(r.procs.len(), 2);
        assert!(r.procs.iter().all(|p| p.borrow().signals == [libc::SIGINT]));
    }

    #[test]
    fn real_processes_are_started_polled_and_stopped() {
        let mut p = popen(&words(&["/bin/sh", "-c", "exit 3"]));
        assert_eq!(p.wait(), Some(3));
        assert_eq!(p.poll(), Some(3));
        let mut p = popen(&words(&["/bin/sh", "-c", "kill -USR1 $$; sleep 5"]));
        assert_eq!(p.wait(), Some(-(libc::SIGUSR1 as i64)));
        let mut p = popen(&words(&["/bin/sleep", "30"]));
        assert_eq!(p.poll(), None);
        assert!(p.pid() > 0);
        p.kill();
        assert_eq!(p.wait(), Some(-9));
    }
}
