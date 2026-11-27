//! What the first game's tests share: the client wired straight to the fake game, and the game switch.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::{json, Value};
use sts2bot::client::GameUnavailable;
use sts2bot::games::sts1::client::{Prefs, Sts1Client, Wire};
use sts2bot::games::{self, Playing};

use super::fake_comm::FakeComm;

/// Sts1Client with the relay taken out: the fake game answers in the same words the relay passes on.
/// (Python's subclass of Sts1Client: here the client's `Wire`, see `direct`.)
pub struct Direct {
    pub fake: Rc<RefCell<FakeComm>>,
}

impl Wire for Direct {
    fn ping(&mut self) -> bool {
        true
    }

    fn _message(&mut self) -> Result<Value, GameUnavailable> {
        Ok(self.fake.borrow().message())
    }

    fn send(&mut self, command: &str) -> Result<Value, GameUnavailable> {
        let answer = self.fake.borrow_mut().command(command);
        Ok(if answer.get("error").is_some() { json!({"status": "error", "error": answer["error"]}) } else { json!({"status": "ok", "message": answer}) })
    }
}

/// `Direct(fake, prefs=...)`.
pub fn direct_with(fake: &Rc<RefCell<FakeComm>>, prefs: Prefs) -> Sts1Client {
    Sts1Client::with_wire(Box::new(Direct { fake: fake.clone() }), prefs)
}

/// `Direct(fake)`.
pub fn direct(fake: &Rc<RefCell<FakeComm>>) -> Sts1Client {
    direct_with(fake, Prefs::None)
}

/// Plays the first game for the length of each test (`let _case = sts1_case();`: Sts1Case's setUp, and its cleanup when dropped).
pub fn sts1_case() -> Playing {
    games::playing("sts1")
}
