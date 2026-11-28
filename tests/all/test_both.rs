//! Both games at once (`--game both`): a bot process for each, kept going; and the first game's CommunicationMod check.
//! (The `Both` class of test_both.rs, which drives `both::run` with a fake clock and fake bots, is in src/games/both.rs.)

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use indexmap::IndexMap;
use sts2bot::games::sts1::host;
use sts2bot::games;
use sts2bot::logging::{self, Level};
use sts2bot::{cli, supervisor};

fn words(w: &[&str]) -> Vec<String> {
    w.iter().map(|x| x.to_string()).collect()
}

/// `with self.assertLogs(logger, level) as said`: what `f` returned and `said.output` (at least one line, or the assertion fails).
fn assert_logs<T>(logger: &str, level: Level, f: impl FnOnce() -> T) -> (T, Vec<String>) {
    let (out, records) = logging::capture(f);
    let said: Vec<String> = records.iter().filter(|r| r.level >= level && (r.logger == logger || r.logger.starts_with(&format!("{logger}.")))).map(|r| r.output()).collect();
    assert!(!said.is_empty(), "no logs of level {} or higher triggered on {}", level.name(), logger);
    (out, said)
}

/// CommandLine.setUp: a directory of its own for CommunicationMod's config (XDG_CONFIG_HOME), no logging set-up,
/// and the game that was being played put back afterwards.
struct CommandLine {
    tmp: tempfile::TempDir,
    _environ: supervisor::Patched,
    _hooks: cli::Patched,
    _game: games::Playing,
}

impl CommandLine {
    fn set_up() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let mut env: IndexMap<String, String> = std::env::vars().collect();
        env.insert("XDG_CONFIG_HOME".to_string(), tmp.path().to_string_lossy().into_owned());
        CommandLine { tmp, _environ: supervisor::patch(|s| s.environ = Some(env)), _hooks: cli::patch(|h| h.basic_config = Some(Rc::new(|_, _| {}))), _game: games::playing("sts2") }
    }

    fn tmp(&self) -> String {
        self.tmp.path().to_string_lossy().into_owned()
    }
}

// CommandLine

#[test]
fn test_options_that_belong_to_one_game_are_refused() {
    let _case = CommandLine::set_up();
    for opt in [["--port", "1"], ["--log-dir", "x"], ["--launch-cmd", "x"]] {
        // (the original's SystemExit: the usage and the error on stderr, exit code 2)
        assert_eq!(cli::main(words(&["--game", "both", opt[0], opt[1]])), 2);
    }
}

#[test]
fn test_both_starts_once_communicationmod_starts_the_relay() {
    let _case = CommandLine::set_up();
    let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
    let seen = Rc::clone(&calls);
    let _run = cli::patch(|h| {
        h.both_run = Some(Rc::new(move |argv| {
            seen.borrow_mut().push(argv.to_vec());
            0
        }))
    });
    let (code, said) = assert_logs("sts2bot", Level::Error, || cli::main(words(&["--game", "both", "-v"])));
    assert_eq!(code, 2); // no config yet: nothing is started
    assert!(said[0].contains("sts1-setup"));
    assert!(calls.borrow().is_empty());
    host::write_comm_config(games::default_port("sts1") as i64, None).unwrap();
    let (code, _) = logging::capture(|| cli::main(words(&["--game", "both", "-v"])));
    assert_eq!(code, 0);
    assert_eq!(*calls.borrow(), vec![words(&["--game", "both", "-v"])]);
}

#[test]
fn test_the_first_game_is_not_launched_for_another_program() {
    let case = CommandLine::set_up();
    std::fs::create_dir_all(host::comm_config_path().parent().unwrap()).unwrap();
    std::fs::write(host::comm_config_path(), "command=otherbot /elsewhere/main --ascension 0\nrunAtGameStart=true\n").unwrap();
    let played = Rc::new(std::cell::Cell::new(false));
    let seen = Rc::clone(&played);
    let _sup = cli::patch(|h| {
        h.play = Some(Rc::new(move |_, _| {
            seen.set(true);
            Ok(())
        }))
    });
    let (code, said) = assert_logs("sts2bot", Level::Error, || cli::main(words(&["--game", "sts1", "--log-dir", &case.tmp()])));
    assert_eq!(code, 2);
    assert!(!played.get());
    assert!(said[0].contains("/elsewhere/main"));
}

// CommConfig

#[test]
fn test_what_communicationmod_starts_is_checked() {
    let tmp = tempfile::tempdir().unwrap();
    let path: PathBuf = tmp.path().join("config.properties");
    assert!(host::comm_config_problem(15527, Some(&path)).unwrap().contains("no command"));
    std::fs::write(&path, "verbose=true\ncommand=otherbot /x/slaythespire/main --ascension 0\n").unwrap();
    assert!(host::comm_config_problem(15527, Some(&path)).unwrap().contains("another program"));
    host::write_comm_config(15599, Some(&path)).unwrap();
    assert!(host::comm_config_problem(15527, Some(&path)).unwrap().contains("another port"));
    assert_eq!(host::comm_config_problem(15599, Some(&path)), None);
    std::fs::write(&path, format!("command={} relay\n", host::executable())).unwrap(); // no --port: the relay's own default
    assert_eq!(host::comm_config_problem(15527, Some(&path)), None);
}
