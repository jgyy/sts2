//! The relay CommunicationMod starts: commands out on stdout, states in on stdin, the bot on localhost HTTP.

use std::cell::RefCell;
use std::io::{BufRead, Read, Write};
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::json;
use sts2bot::bot::BotArgs;
use sts2bot::games::sts1::client::Sts1Client;
use sts2bot::games::sts1::relay::{self as relay_mod, Relay};
use sts2bot::base::{self, JsonExt};

use super::fake_comm::FakeComm;
use super::helpers::sts1_case;
use crate::helpers::make_bot_kw;

/// The relay's stdout, read by a fake game that answers on the relay's stdin a moment later.
#[derive(Clone)]
struct GamePipe {
    game: Arc<Mutex<FakeComm>>,
    relay: Arc<OnceLock<Arc<Relay>>>,
}

impl Write for GamePipe {
    fn write(&mut self, s: &[u8]) -> std::io::Result<usize> {
        for line in String::from_utf8_lossy(s).lines() {
            if !line.trim().is_empty() && line.trim() != "ready" {
                let answer = base::json_dumps(&self.game.lock().unwrap().command(line.trim()));
                let relay = self.relay.get().expect("the relay this pipe belongs to").clone();
                std::thread::spawn(move || relay.feed(&answer));
            }
        }
        Ok(s.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn relay_to(game: &Arc<Mutex<FakeComm>>, timeout: f64) -> Arc<Relay> {
    let pipe = GamePipe { game: game.clone(), relay: Arc::new(OnceLock::new()) };
    let relay = Arc::new(Relay::new(Some(Box::new(pipe.clone())), timeout));
    let _ = pipe.relay.set(relay.clone());
    relay
}

// class RelayTests

#[test]
fn test_a_command_is_answered_with_the_games_next_message() {
    let _case = sts1_case();
    let game = Arc::new(Mutex::new(FakeComm::default()));
    let relay = relay_to(&game, 2.0);
    relay.feed(&base::json_dumps(&game.lock().unwrap().message()));
    assert_eq!(relay.snapshot()["message"]["in_game"], false);
    let answer = relay.send("start IRONCLAD 0", None);
    assert_eq!(answer["status"], "ok");
    assert!(answer["message"]["in_game"].truthy());
    let bad = relay.send("end", None); // (not on the map)
    assert_eq!(bad["status"], "error");
    assert!(bad["error"].s().contains("Invalid command"));
    assert!(relay.snapshot()["message"]["in_game"].truthy()); // an error does not replace the state
}

#[test]
fn test_a_game_that_does_not_answer_leaves_the_command_pending() {
    let _case = sts1_case();
    let relay = Relay::new(Some(Box::new(Vec::new())), 0.05);
    assert!(relay.send("state", None)["pending"].truthy());
    relay.feed("not json"); // (ignored; its line on stderr is the test harness's to keep)
    relay.feed(&base::json_dumps(&json!({"in_game": false, "available_commands": ["start"]})));
    assert_eq!(relay.snapshot()["seq"], 1);
}

#[test]
fn test_the_bot_plays_a_run_through_the_relay_over_http() {
    let _case = sts1_case();
    let game = Arc::new(Mutex::new(FakeComm::seeded(5)));
    let relay = relay_to(&game, 5.0);
    let server = relay_mod::serve(0, relay.clone(), 30).unwrap();
    let serving = server.clone();
    std::thread::spawn(move || serving.serve_forever());
    /// `self.addCleanup(server.shutdown)`, `self.addCleanup(server.server_close)`.
    struct Cleanup(Arc<relay_mod::Server>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            self.0.shutdown();
            self.0.server_close();
        }
    }
    let _cleanup = Cleanup(server.clone());
    let client = Rc::new(RefCell::new(Sts1Client::on_port(server.server_address.port())));
    assert!(!client.borrow_mut().ping()); // up, but the game has not said anything yet
    relay.feed(&base::json_dumps(&game.lock().unwrap().message()));
    assert!(client.borrow_mut().ping());
    let mut bot = make_bot_kw(client, BotArgs { character: "WATCHER".to_string(), ..Default::default() });
    bot.run(Some(1)).unwrap();
    assert_eq!(bot.runs_finished, 1);
    assert!(game.lock().unwrap().sent.iter().any(|s| s == "start WATCHER 0"));
}

/// The relay's stdin as a pipe: what the test writes, and the end of it when the test lets go.
struct PipeIn(mpsc::Receiver<Vec<u8>>, Vec<u8>);

impl Read for PipeIn {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.1.is_empty() {
            match self.0.recv() {
                Ok(more) => self.1 = more,
                Err(_) => return Ok(0), // closed
            }
        }
        let n = buf.len().min(self.1.len());
        buf[..n].copy_from_slice(&self.1[..n]);
        self.1.drain(..n);
        Ok(n)
    }
}

/// The relay's stdout as a pipe.
struct PipeOut(mpsc::Sender<Vec<u8>>);

impl Write for PipeOut {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = self.0.send(buf.to_vec());
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn test_the_relay_says_ready_and_leaves_with_the_game() {
    let _case = sts1_case();
    // (the original starts relay.rs as a process; here the relay's life, `run`, with a pipe at each end: the process around it,
    // `sts2bot relay`, is the command line's.)
    let (stdin, relay_stdin) = mpsc::channel::<Vec<u8>>();
    let (relay_stdout, stdout) = mpsc::channel::<Vec<u8>>();
    let input: Box<dyn BufRead + Send> = Box::new(std::io::BufReader::new(PipeIn(relay_stdin, Vec::new())));
    let proc = std::thread::spawn(move || relay_mod::run(0, input, Box::new(PipeOut(relay_stdout))));
    let mut line = Vec::new();
    while !line.contains(&b'\n') {
        line.extend(stdout.recv_timeout(Duration::from_secs(10)).expect("a line from the relay"));
    }
    assert_eq!(String::from_utf8_lossy(&line).lines().next().unwrap().trim(), "ready"); // what CommunicationMod waits for
    drop(stdin); // the game is gone
    let (done, waited) = mpsc::channel();
    std::thread::spawn(move || done.send(proc.join().unwrap()));
    assert_eq!(waited.recv_timeout(Duration::from_secs(10)).expect("the relay to leave"), 0);
}
