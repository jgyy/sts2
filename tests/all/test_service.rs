// TODO(cli): these go through the command line (`sts2bot::cli::main`) and wait for it:
//   - test_the_cli_sends_service_commands_to_the_service_not_the_bot
//     (cli::main(["service", "disable"]) == 0 with `service::patch_run` standing in for systemctl, which must have been asked
//     exactly once for ["systemctl", "--user", "disable", "--now", "sts2bot"])

use std::cell::RefCell;
use std::rc::Rc;

use sts2bot::service::{self, Printed, RunPatch};

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

/// Never let a test reach the real systemctl: that would stop the live bot.
fn no_systemctl() -> RunPatch {
    service::patch_run(|_| panic!("tests must not run systemctl"))
}

#[test]
fn test_disable_stops_the_unit_and_keeps_it_from_starting_at_login() {
    let _guard = no_systemctl();
    let mut calls: Vec<Vec<String>> = Vec::new();
    let mut runner = |a: &[String]| {
        calls.push(a.to_vec());
        Ok(0)
    };
    let (code, _printed) = service::capture_print(|| service::main(&args(&["disable"]), Some(&mut runner)));
    assert_eq!(code, 0);
    assert_eq!(calls, vec![args(&["systemctl", "--user", "disable", "--now", "sts2bot"])]);
}

#[test]
fn test_each_action_maps_to_its_systemctl_command() {
    let expected: [(&str, &[&str]); 5] = [
        ("status", &["systemctl", "--user", "status", "--no-pager", "--lines=0", "sts2bot"]),
        ("start", &["systemctl", "--user", "start", "sts2bot"]),
        ("stop", &["systemctl", "--user", "stop", "sts2bot"]),
        ("enable", &["systemctl", "--user", "enable", "--now", "sts2bot"]),
        ("disable", &["systemctl", "--user", "disable", "--now", "sts2bot"]),
    ];
    for (action, want) in expected {
        assert_eq!(service::systemctl_args(action, service::UNIT), args(want));
    }
}

#[test]
fn test_a_failing_systemctl_gives_its_exit_code_and_no_success_note() {
    let _guard = no_systemctl();
    let (code, printed) = service::capture_print(|| service::main(&args(&["disable"]), Some(&mut |_: &[String]| Ok(5))));
    assert_eq!(code, 5);
    assert_eq!(printed, Printed::default());
}

#[test]
fn test_unknown_action_is_refused() {
    let _guard = no_systemctl();
    let mut ran = false;
    let mut runner = |_: &[String]| {
        ran = true;
        Ok(0)
    };
    // (Python's argparse left with SystemExit(2); here that is the code returned, and nothing was run)
    let (code, printed) = service::capture_print(|| service::main(&args(&["restart"]), Some(&mut runner)));
    assert_eq!(code, 2);
    assert!(!ran);
    assert!(printed.stderr.contains("invalid choice: 'restart'"));
}

#[test]
fn test_a_missing_systemctl_says_so() {
    let _guard = no_systemctl();
    let mut runner = |_: &[String]| Err(std::io::Error::new(std::io::ErrorKind::NotFound, "systemctl"));
    let (code, printed) = service::capture_print(|| service::main(&args(&["stop"]), Some(&mut runner)));
    assert_eq!(code, 127);
    assert!(printed.stderr.contains("systemctl was not found"));
    assert_eq!(printed.stdout, "");
}

/// Without a runner of its own, `main` runs commands the thread's way: the stand-in a test put there.
#[test]
fn test_the_stand_in_for_running_commands_is_used() {
    let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
    let seen = calls.clone();
    let _guard = service::patch_run(move |a| {
        seen.borrow_mut().push(a.to_vec());
        Ok(0)
    });
    let (code, printed) = service::capture_print(|| service::main(&args(&["enable", "--unit", "other"]), None));
    assert_eq!(code, 0);
    assert_eq!(*calls.borrow(), vec![args(&["systemctl", "--user", "enable", "--now", "other"])]);
    assert_eq!(printed.stdout, "The bot is started now and at every login.\n");
}
