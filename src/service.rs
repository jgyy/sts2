//! `sts2bot service ...`: turn the bot's systemd user service on or off (killing the bot does not: the unit restarts it).

use std::cell::RefCell;

use crate::py;

pub const UNIT: &str = "sts2bot"; // systemd/sts2bot.service, installed as the user unit of the same name

/// What each action asks systemctl to do (`stop` lasts until the next login; `disable` also keeps it from starting then).
pub const ACTIONS: [(&str, &[&str]); 5] =
    [("status", &["status", "--no-pager", "--lines=0"]), ("start", &["start"]), ("stop", &["stop"]), ("enable", &["enable", "--now"]), ("disable", &["disable", "--now"])];
pub const NOTE: [(&str, &str); 3] = [
    ("stop", "The bot is stopped until you start it again or log in next time."),
    ("disable", "The bot is stopped and will not start at login. Start it again with: sts2bot service enable"),
    ("enable", "The bot is started now and at every login."),
];

/// What runs a command line and gives its exit code (an error of kind NotFound: there is no such program).
pub type Runner = dyn FnMut(&[String]) -> std::io::Result<i32>;

/// What a test caught of the lines `main` printed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Printed {
    pub stdout: String,
    pub stderr: String,
}

thread_local! {
    static RUN: RefCell<Option<Box<Runner>>> = const { RefCell::new(None) };
    static PRINTED: RefCell<Option<Printed>> = const { RefCell::new(None) };
}

/// Puts the earlier way of running commands back when dropped.
#[must_use = "the stand-in ends when this is dropped"]
pub struct RunPatch(Option<Box<Runner>>);

impl Drop for RunPatch {
    fn drop(&mut self) {
        let before = self.0.take();
        RUN.with(|r| *r.borrow_mut() = before);
    }
}

/// A stand-in for running commands on this thread (tests: so that none reaches the real systemctl), until the guard is dropped.
pub fn patch_run(run: impl FnMut(&[String]) -> std::io::Result<i32> + 'static) -> RunPatch {
    RunPatch(RUN.with(|r| r.borrow_mut().replace(Box::new(run))))
}

/// Run `f` and return what it returned with what this thread printed meanwhile (nothing reaches the terminal).
pub fn capture_print<T>(f: impl FnOnce() -> T) -> (T, Printed) {
    let before = PRINTED.with(|p| p.borrow_mut().replace(Printed::default()));
    let out = f();
    let printed = PRINTED.with(|p| std::mem::replace(&mut *p.borrow_mut(), before)).unwrap_or_default();
    (out, printed)
}

fn print(text: &str, to_stderr: bool) {
    let caught = PRINTED.with(|p| match p.borrow_mut().as_mut() {
        Some(printed) => {
            let buffer = if to_stderr { &mut printed.stderr } else { &mut printed.stdout };
            buffer.push_str(text);
            buffer.push('\n');
            true
        }
        None => false,
    });
    if caught {
    } else if to_stderr {
        eprintln!("{text}");
    } else {
        println!("{text}");
    }
}

/// `subprocess.run(args).returncode`: the exit code, or minus the signal that ended it.
pub fn subprocess_run(args: &[String]) -> std::io::Result<i32> {
    use std::os::unix::process::ExitStatusExt;
    let status = std::process::Command::new(&args[0]).args(&args[1..]).status()?;
    Ok(status.code().unwrap_or_else(|| -status.signal().unwrap_or(0)))
}

pub fn systemctl_args(action: &str, unit: &str) -> Vec<String> {
    let words = ACTIONS.iter().find(|(a, _)| *a == action).unwrap_or_else(|| panic!("KeyError: {}", py::str_repr(action))).1;
    let mut out = vec!["systemctl".to_string(), "--user".to_string()];
    out.extend(words.iter().map(|w| w.to_string()));
    out.push(unit.to_string());
    out
}

fn choices() -> Vec<&'static str> {
    let mut names: Vec<&str> = ACTIONS.iter().map(|(a, _)| *a).collect();
    names.sort();
    names
}

fn usage() -> String {
    format!("usage: sts2bot service [-h] [--unit UNIT] {{{}}}", choices().join(","))
}

enum Parsed {
    Args { action: String, unit: String },
    /// The parser left by itself (after --help, or a command line it could not read): the exit code.
    Exit(i32),
}

/// The command line as Python's argparse read it: one action, `--unit UNIT` (or `--unit=UNIT`, or a prefix of it), `-h`.
fn parse_args(argv: &[String]) -> Parsed {
    const LONG: [&str; 2] = ["--help", "--unit"];
    let error = |message: String| {
        print(&usage(), true);
        print(&format!("sts2bot service: error: {message}"), true);
        Parsed::Exit(2)
    };
    // An option (which, and what follows its "="), or a plain word.
    let option = |word: &str| -> Option<(Option<&'static str>, Option<String>)> {
        if !word.starts_with('-') || word.len() == 1 {
            return None;
        }
        if word == "-h" {
            return Some((Some("--help"), None));
        }
        let (name, explicit) = match word.split_once('=') {
            Some((n, e)) => (n, Some(e.to_string())),
            None => (word, None),
        };
        if let Some(full) = LONG.iter().find(|o| **o == name) {
            return Some((Some(full), explicit));
        }
        if name.starts_with("--") {
            let found: Vec<&&str> = LONG.iter().filter(|o| o.starts_with(name)).collect();
            if found.len() == 1 {
                return Some((Some(found[0]), explicit));
            }
        }
        let number = word[1..].chars().all(|c| c.is_ascii_digit() || c == '.') && word[1..].chars().any(|c| c.is_ascii_digit());
        if number || word.contains(' ') {
            return None;
        }
        Some((None, None))
    };
    let (mut action, mut unit): (Option<String>, String) = (None, UNIT.to_string());
    let mut extras: Vec<String> = Vec::new();
    let mut plain = false; // after "--": nothing is an option
    let mut i = 0;
    while i < argv.len() {
        let word = argv[i].as_str();
        i += 1;
        if !plain && word == "--" {
            plain = true;
            continue;
        }
        match if plain { None } else { option(word) } {
            None => {
                if action.is_some() {
                    extras.push(word.to_string());
                } else if choices().contains(&word) {
                    action = Some(word.to_string());
                } else {
                    let all: Vec<String> = choices().iter().map(|c| py::str_repr(c)).collect();
                    return error(format!("argument action: invalid choice: {} (choose from {})", py::str_repr(word), all.join(", ")));
                }
            }
            Some((None, _)) => extras.push(word.to_string()),
            Some((Some("--help"), explicit)) => {
                if let Some(explicit) = explicit {
                    return error(format!("argument -h/--help: ignored explicit argument {}", py::str_repr(&explicit)));
                }
                print(&format!("{}\n\nStart, stop, enable or disable the bot's systemd user service.\n\npositional arguments:\n  {{{}}}\n\noptions:\n  -h, --help            show this help message and exit\n  --unit UNIT           unit name (default: {})", usage(), choices().join(","), UNIT), false);
                return Parsed::Exit(0);
            }
            Some((Some(_), Some(explicit))) => unit = explicit,
            Some((Some(_), None)) => match argv.get(i) {
                Some(value) if value != "--" && option(value).is_none() => {
                    unit = value.clone();
                    i += 1;
                }
                _ => return error("argument --unit: expected one argument".to_string()),
            },
        }
    }
    match action {
        None => error("the following arguments are required: action".to_string()),
        Some(_) if !extras.is_empty() => error(format!("unrecognized arguments: {}", extras.join(" "))),
        Some(action) => Parsed::Args { action, unit },
    }
}

/// `runner`: what runs the command instead of the thread's own way (`patch_run`, or else `subprocess_run`).
/// Where Python's argparse left with SystemExit (a command line it could not read: 2; --help: 0), this returns that code.
pub fn main(argv: &[String], runner: Option<&mut (dyn FnMut(&[String]) -> std::io::Result<i32> + '_)>) -> i32 {
    let (action, unit) = match parse_args(argv) {
        Parsed::Args { action, unit } => (action, unit),
        Parsed::Exit(code) => return code,
    };
    let args = systemctl_args(&action, &unit);
    // looked up now, so a stand-in can replace it
    let ran = match runner {
        Some(run) => run(&args),
        None => {
            let patched = RUN.with(|r| r.borrow_mut().take());
            match patched {
                Some(mut run) => {
                    let ran = run(&args);
                    RUN.with(|r| {
                        let mut slot = r.borrow_mut();
                        if slot.is_none() {
                            *slot = Some(run);
                        }
                    });
                    ran
                }
                None => subprocess_run(&args),
            }
        }
    };
    let code = match ran {
        Ok(code) => code,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            print("systemctl was not found: this only works on a systemd Linux desktop", true);
            return 127;
        }
        Err(e) => panic!("could not run {}: {}", args.join(" "), e),
    };
    if let Some((_, note)) = NOTE.iter().find(|(a, _)| *a == action).filter(|_| code == 0) {
        print(note, false);
        if action == "stop" || action == "disable" {
            print("The game keeps running; close it from the game or Steam if you want it gone too.", false);
        }
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::py::JsonExt;
    use serde_json::Value;

    /// Every command line of the golden file: the exit code, what systemctl was asked and what was printed, as Python's.
    #[test]
    fn command_lines_as_python() {
        let golden: Value = serde_json::from_str(include_str!("../tests/golden/service.json")).unwrap();
        let _never = patch_run(|args| panic!("tests must not run systemctl ({args:?})"));
        assert!(golden.list().len() > 100);
        for case in golden.list() {
            let argv: Vec<String> = case["argv"].list().iter().map(|a| a.s().to_string()).collect();
            let mut calls: Vec<Vec<String>> = Vec::new();
            let mut runner = |args: &[String]| {
                calls.push(args.to_vec());
                match &case["rc"] {
                    Value::String(_) => Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
                    rc => Ok(rc.int() as i32),
                }
            };
            let (code, printed) = capture_print(|| main(&argv, Some(&mut runner)));
            assert_eq!(code as i64, case["code"].int(), "{case}");
            assert_eq!(serde_json::json!(calls), case["calls"], "{case}");
            assert_eq!(printed.stdout, case["stdout"].s().replace("python3 -m sts2bot", "sts2bot"), "{case}");
            assert_eq!(printed.stderr, case["stderr"].s(), "{case}");
        }
    }
}
