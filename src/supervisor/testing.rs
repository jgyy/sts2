//! What the golden tests of this module and of games/sts1/host.rs share.

use super::*;

/// What `f` returned, and the lines logged on sts2bot.supervisor meanwhile as `[level, message]`.
pub fn logged<T>(f: impl FnOnce() -> T) -> (T, Vec<Value>) {
    let (out, records) = crate::logging::capture(f);
    (out, records.iter().filter(|r| r.logger == "sts2bot.supervisor").map(|r| json!([r.level.name(), r.message])).collect())
}

/// The environment the golden cases were made in, and `more` (a null value: not set).
pub fn env(more: &[(&str, Value)]) -> IndexMap<String, String> {
    let mut env: IndexMap<String, String> = IndexMap::new();
    env.insert("HOME".to_string(), "/nonexistent-home".to_string());
    env.insert("PATH".to_string(), "/usr/bin:/bin".to_string());
    for (k, v) in more {
        if let Some(v) = v.as_str() {
            env.insert(k.to_string(), v.to_string());
        }
    }
    env
}

/// The command that starts the relay, as this program writes it (the golden files have `{RELAY}` in its place).
pub fn relay() -> String {
    format!("{} relay", crate::games::sts1::host::executable())
}

/// The value with the temporary directory written `<T>` and the relay's command `{RELAY}`, as the golden files have them.
pub fn norm(v: Value, tmp: &Path) -> Value {
    serde_json::from_str(&v.to_string().replace(&*tmp.to_string_lossy(), "<T>").replace(&relay(), "{RELAY}")).unwrap()
}

pub fn words(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|w| w.as_str().unwrap().to_string()).collect()
}

/// A process that never ends.
pub struct Running;

impl Proc for Running {
    fn pid(&self) -> i64 {
        1
    }
    fn poll(&mut self) -> Option<i64> {
        None
    }
    fn send_signal(&mut self, _sig: i32) {}
    fn kill(&mut self) {}
    fn wait(&mut self) -> Option<i64> {
        None
    }
}
