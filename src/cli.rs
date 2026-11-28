//! Command line entry point: `sts2bot`.
//!
//! The command line is read as the original's argparse read it (the same options, abbreviations, usage, help and error
//! texts, exit codes): `parse_args`, over the small `Parser` below.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::bot::{Bot, BotArgs, SharedGame};
use crate::client::Client;
use crate::games::both::{self, KeyboardInterrupt};
use crate::games::sts1::client::{Prefs, Sts1Client};
use crate::games::sts1::{host as sts1_host, relay};
use crate::games::{self};
use crate::logging::{self, Handler, Level, Logger, RotatingFile, Sink};
use crate::supervisor::{self, Host, Supervisor, SupervisorArgs, DEFAULT_LAUNCH, FAST_MODES};
use crate::{learn, log_error, log_info, log_warning, base, service};

#[allow(non_upper_case_globals)]
pub static log: Logger = Logger::new("sts2bot");

// (x3 to x6 made no difference while the bot's 0.2 s waits and the mod's 27 ms state builds held up every turn; with both cut, 2026-11-25: x6;
// 2026-11-26, the user: "x16 the speed for both sts1 and sts2" (the mod allows up to 20).)
pub const DEFAULT_TIME_SCALE: f64 = 16.0;
// The first game's clock, through our own mod (games/sts1/speedmod.rs). It runs at 240 frames a second there, so even x10
// leaves each frame the time step of a 24 fps game: the game's logic sees nothing unusual. (x8 until 2026-11-25: its enemy
// turns, 0.5-0.7 s, were then most of the time a round takes.)
pub const DEFAULT_STS1_TIME_SCALE: f64 = 16.0; // (x16 from 2026-11-26, the user's ask; speedmod.rs allows up to 16: a frame's step is then a 15 fps game's)
pub const DATE_FORMAT: &str = "%Y-%m-%d %H:%M:%S";
pub const STS1_SETUP_HINT: &str = "would wait for a relay that never starts and restart the game for ever. Run `sts2bot sts1-setup` to point CommunicationMod at the bot's relay";

// ---------------------------------------------------------------------------------------------------------------------
// What tests put in the place of the parts of `main` that reach outside it (the original's mock.patch of logging.basicConfig,
// both.run and Supervisor).

pub type BasicConfig = dyn Fn(Level, Vec<Handler>);
pub type BothRun = dyn Fn(&[String]) -> i32;
pub type Play = dyn Fn(&mut Supervisor, Option<i64>) -> Result<(), KeyboardInterrupt>;

/// The stand-ins in force on this thread (None: the real thing). See `patch`.
#[derive(Clone, Default)]
pub struct Hooks {
    /// `logging.basicConfig(level=..., handlers=...)`
    pub basic_config: Option<Rc<BasicConfig>>,
    /// `both.run(raw)`
    pub both_run: Option<Rc<BothRun>>,
    /// `sup.run(args.runs)`, with the supervisor `main` made
    pub play: Option<Rc<Play>>,
}

thread_local! {
    static HOOKS: RefCell<Hooks> = RefCell::new(Hooks::default());
}

/// Puts the earlier hooks back when dropped.
#[must_use = "the stand-ins end when this is dropped"]
pub struct Patched(Option<Hooks>);

impl Drop for Patched {
    fn drop(&mut self) {
        if let Some(before) = self.0.take() {
            HOOKS.with(|h| *h.borrow_mut() = before);
        }
    }
}

/// Stand-ins on this thread until the guard is dropped: `let _p = cli::patch(|h| h.both_run = Some(Rc::new(|_| 0)));`
pub fn patch(change: impl FnOnce(&mut Hooks)) -> Patched {
    let before = HOOKS.with(|h| h.borrow().clone());
    HOOKS.with(|h| change(&mut h.borrow_mut()));
    Patched(Some(before))
}

fn basic_config(level: Level, handlers: Vec<Handler>) {
    match HOOKS.with(|h| h.borrow().basic_config.clone()) {
        Some(hook) => hook(level, handlers),
        None => logging::configure(level, handlers),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// argparse.

/// Where the parser left by itself (the original's SystemExit): after --help (0, the help on stdout) or a command line it
/// could not read (2, the usage and the error on stderr).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Exit {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Exit {
    /// Print what was to be printed; the exit code.
    pub fn leave(&self) -> i32 {
        // (as argparse's `_print_message`: a closed stdout or stderr is not an error)
        use std::io::Write;
        let _ = std::io::stdout().write_all(self.stdout.as_bytes()).and_then(|_| std::io::stdout().flush());
        let _ = std::io::stderr().write_all(self.stderr.as_bytes());
        self.code
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// `action="store_true"`
    Flag,
    /// `action="help"`
    Help,
    Str,
    Int,
    Float,
    Path,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    None,
    Bool(bool),
    Str(String),
    Int(i64),
    Float(f64),
}

#[derive(Clone, Debug)]
pub struct Opt {
    pub flags: Vec<&'static str>,
    pub kind: Kind,
    pub default: Val,
    pub choices: Vec<&'static str>,
    pub metavar: Option<&'static str>,
    pub help: Option<String>,
}

impl Opt {
    pub fn new(flags: &[&'static str], kind: Kind, default: Val) -> Self {
        Opt { flags: flags.to_vec(), kind, default, choices: Vec::new(), metavar: None, help: None }
    }

    pub fn help(mut self, text: impl Into<String>) -> Self {
        self.help = Some(text.into());
        self
    }

    pub fn choices(mut self, choices: &[&'static str]) -> Self {
        self.choices = choices.to_vec();
        self
    }

    pub fn metavar(mut self, metavar: &'static str) -> Self {
        self.metavar = Some(metavar);
        self
    }

    /// `--sts1-time-scale` -> `sts1_time_scale` (the first long flag's, else the first flag's).
    pub fn dest(&self) -> String {
        let flag = self.flags.iter().find(|f| f.starts_with("--")).unwrap_or(&self.flags[0]);
        flag.trim_start_matches('-').replace('-', "_")
    }

    fn takes_value(&self) -> bool {
        !matches!(self.kind, Kind::Flag | Kind::Help)
    }

    fn name(&self) -> String {
        self.flags.join("/")
    }

    fn metavar_text(&self) -> String {
        match self.metavar {
            Some(m) => m.to_string(),
            None if !self.choices.is_empty() => format!("{{{}}}", self.choices.join(",")),
            None => self.dest().to_uppercase(),
        }
    }

    fn invocation(&self) -> String {
        if self.takes_value() {
            format!("{} {}", self.flags.join(", "), self.metavar_text())
        } else {
            self.flags.join(", ")
        }
    }

    fn usage_part(&self) -> String {
        if self.takes_value() {
            format!("[{} {}]", self.flags[0], self.metavar_text())
        } else {
            format!("[{}]", self.flags[0])
        }
    }
}

/// `shutil.get_terminal_size().columns`.
fn terminal_columns() -> usize {
    if let Some(columns) = supervisor::environ_get("COLUMNS").and_then(|c| c.trim().parse::<i64>().ok()).filter(|c| *c > 0) {
        return columns as usize;
    }
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut size) } == 0 && size.ws_col > 0 {
        return size.ws_col as usize;
    }
    80
}

fn len(s: &str) -> usize {
    s.chars().count()
}

/// `TextWrapper._split`: words and the blanks between them, a hyphenated word cut after its hyphens.
fn split_chunks(text: &str) -> Vec<String> {
    const WS: &str = r"[\t\n\x0b\x0c\r ]";
    const NWS: &str = r"[^\t\n\x0b\x0c\r ]";
    const WP: &str = r#"[A-Za-z0-9_!"'&.,?]"#;
    const LT: &str = r"[A-Za-z_]";
    let pattern = format!(r"{WS}+|(?<={WP})-{{2,}}(?=\w)|{NWS}+?(?:-(?:(?<={LT}{LT}-)|(?<={LT}-{LT}-))(?={LT}-?{LT})|(?={WS}|\z)|(?<={WP})(?=-{{2,}}\w))");
    let re = base::re::cached_fancy(&pattern);
    let mut chunks = Vec::new();
    let mut last = 0;
    for found in re.find_iter(text).flatten() {
        if found.start() > last {
            chunks.push(text[last..found.start()].to_string());
        }
        if found.end() > found.start() {
            chunks.push(found.as_str().to_string());
        }
        last = found.end();
    }
    if last < text.len() {
        chunks.push(text[last..].to_string());
    }
    chunks
}

/// `textwrap.wrap(text, width)`.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut chunks = split_chunks(text);
    chunks.reverse();
    let blank = |s: &str| s.trim().is_empty();
    let mut lines: Vec<String> = Vec::new();
    while !chunks.is_empty() {
        let mut cur_line: Vec<String> = Vec::new();
        let mut cur_len = 0;
        // First chunk on line is whitespace -- drop it, unless this is the very beginning of the text.
        if blank(chunks.last().unwrap()) && !lines.is_empty() {
            chunks.pop();
        }
        while let Some(chunk) = chunks.last() {
            let l = len(chunk);
            if cur_len + l <= width {
                cur_len += l;
                cur_line.push(chunks.pop().unwrap());
            } else {
                break;
            }
        }
        // The current line is full, and the next chunk is too big to fit on *any* line (not just this one).
        if chunks.last().is_some_and(|c| len(c) > width) {
            let space_left = if width < 1 { 1 } else { width.saturating_sub(cur_len) };
            if space_left > 0 {
                let chunk: Vec<char> = chunks.pop().unwrap().chars().collect();
                let mut end = space_left;
                if chunk.len() > space_left {
                    // break after last hyphen, but only if there are non-hyphens before it
                    if let Some(hyphen) = chunk[..space_left].iter().rposition(|c| *c == '-') {
                        if hyphen > 0 && chunk[..hyphen].iter().any(|c| *c != '-') {
                            end = hyphen + 1;
                        }
                    }
                }
                cur_line.push(chunk[..end].iter().collect());
                chunks.push(chunk[end..].iter().collect());
            }
        }
        // If the last chunk on this line is all whitespace, drop it.
        if cur_line.last().is_some_and(|c| blank(c)) {
            cur_line.pop();
        }
        if !cur_line.is_empty() {
            lines.push(cur_line.concat());
        }
    }
    lines
}

/// The formatter's `_whitespace_matcher.sub(' ', text).strip()`.
fn one_line(text: &str) -> String {
    text.split(|c: char| " \t\n\r\x0b\x0c".contains(c)).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

pub struct Parser {
    pub prog: &'static str,
    pub description: &'static str,
    pub opts: Vec<Opt>,
}

impl Parser {
    pub fn new(prog: &'static str, description: &'static str) -> Self {
        Parser { prog, description, opts: vec![Opt::new(&["-h", "--help"], Kind::Help, Val::None).help("show this help message and exit")] }
    }

    pub fn add(&mut self, opt: Opt) {
        self.opts.push(opt);
    }

    /// The formatter's `_format_usage`: "usage: prog [-h] ...", wrapped to the terminal.
    fn usage_text(&self, width: usize) -> String {
        let prefix = "usage: ";
        let parts: Vec<String> = self.opts.iter().map(|o| o.usage_part()).collect();
        let mut usage = self.prog.to_string();
        if !parts.is_empty() {
            usage = format!("{} {}", self.prog, parts.join(" "));
        }
        if len(prefix) + len(&usage) > width {
            let get_lines = |parts: &[String], indent: &str, first: Option<usize>| -> Vec<String> {
                let mut lines: Vec<String> = Vec::new();
                let mut line: Vec<&str> = Vec::new();
                let indent_length = len(indent) as i64;
                let mut line_len: i64 = match first {
                    Some(prefix_length) => prefix_length as i64 - 1,
                    None => indent_length - 1,
                };
                for part in parts {
                    if line_len + 1 + len(part) as i64 > width as i64 && !line.is_empty() {
                        lines.push(format!("{indent}{}", line.join(" ")));
                        line.clear();
                        line_len = indent_length - 1;
                    }
                    line.push(part);
                    line_len += len(part) as i64 + 1;
                }
                if !line.is_empty() {
                    lines.push(format!("{indent}{}", line.join(" ")));
                }
                if first.is_some() && !lines.is_empty() {
                    lines[0] = lines[0].chars().skip(indent_length as usize).collect();
                }
                lines
            };
            let lines = if (len(prefix) + len(self.prog)) as f64 <= 0.75 * width as f64 {
                // if prog is short, follow it with optionals
                let indent = " ".repeat(len(prefix) + len(self.prog) + 1);
                if parts.is_empty() {
                    vec![self.prog.to_string()]
                } else {
                    let mut all = vec![self.prog.to_string()];
                    all.extend(parts.iter().cloned());
                    get_lines(&all, &indent, Some(len(prefix)))
                }
            } else {
                // if prog is long, put it on its own line
                let indent = " ".repeat(len(prefix));
                let mut lines = vec![self.prog.to_string()];
                lines.extend(get_lines(&parts, &indent, None));
                lines
            };
            usage = lines.join("\n");
        }
        format!("{prefix}{usage}\n\n")
    }

    /// `format_usage()`.
    pub fn format_usage(&self) -> String {
        let width = terminal_columns().saturating_sub(2);
        format!("{}\n", self.usage_text(width).trim_matches('\n'))
    }

    /// `format_help()`.
    pub fn format_help(&self) -> String {
        let width = terminal_columns() as i64 - 2;
        let max_help_position = 24.min((width - 20).max(4));
        let mut out = self.usage_text(width.max(0) as usize);
        if !self.description.is_empty() {
            out.push_str(&wrap(&one_line(self.description), width.max(11) as usize).join("\n"));
            out.push_str("\n\n");
        }
        // the "options" section, its items indented by 2
        let indent = 2;
        let action_max_length = self.opts.iter().map(|o| len(&o.invocation()) as i64 + indent).max().unwrap_or(0);
        let help_position = (action_max_length + 2).min(max_help_position);
        let help_width = (width - help_position).max(11) as usize;
        let action_width = help_position - indent - 2;
        let pad = |n: i64| " ".repeat(n.max(0) as usize);
        let mut items = String::new();
        for opt in &self.opts {
            let header = opt.invocation();
            let help = opt.help.as_deref().filter(|h| !h.is_empty());
            let indent_first;
            match help {
                None => {
                    items.push_str(&format!("{}{}\n", pad(indent), header));
                    indent_first = 0;
                }
                Some(_) if len(&header) as i64 <= action_width => {
                    items.push_str(&format!("{}{:<w$}  ", pad(indent), header, w = action_width.max(0) as usize));
                    indent_first = 0;
                }
                Some(_) => {
                    items.push_str(&format!("{}{}\n", pad(indent), header));
                    indent_first = help_position;
                }
            }
            if let Some(help) = help.filter(|h| !h.trim().is_empty()) {
                let lines = wrap(&one_line(help), help_width);
                if let Some(first) = lines.first() {
                    items.push_str(&format!("{}{}\n", pad(indent_first), first));
                    for line in &lines[1..] {
                        items.push_str(&format!("{}{}\n", pad(help_position), line));
                    }
                }
            } else if help.is_some() {
                items.push('\n');
            }
        }
        out.push_str("\noptions:\n");
        out.push_str(&items);
        out.push('\n');
        // (the formatter's `_long_break_matcher` and its last strip)
        let out = crate::re!(r"\n\n\n+").replace_all(&out, "\n\n");
        format!("{}\n", out.trim_matches('\n'))
    }

    /// `ap.error(message)`: the usage and the message on stderr, exit code 2.
    pub fn error(&self, message: &str) -> Exit {
        Exit { code: 2, stdout: String::new(), stderr: format!("{}{}: error: {}\n", self.format_usage(), self.prog, message) }
    }

    fn flag(&self, flag: &str) -> Option<usize> {
        self.opts.iter().position(|o| o.flags.contains(&flag))
    }

    /// `_parse_optional`: None for a plain word; else what the word may be as an option:
    /// (which option (None: unknown), its flag, the separator, the explicit argument).
    #[allow(clippy::type_complexity)]
    fn parse_optional(&self, arg: &str) -> Option<Vec<(Option<usize>, String, Option<String>, Option<String>)>> {
        if !arg.starts_with('-') {
            return None;
        }
        if let Some(at) = self.flag(arg) {
            return Some(vec![(Some(at), arg.to_string(), None, None)]);
        }
        if len(arg) == 1 {
            return None;
        }
        let (option_prefix, sep, explicit) = match arg.split_once('=') {
            Some((name, value)) => (name, Some("=".to_string()), Some(value.to_string())),
            None => (arg, None, None),
        };
        if sep.is_some() {
            if let Some(at) = self.flag(option_prefix) {
                return Some(vec![(Some(at), option_prefix.to_string(), sep, explicit)]);
            }
        }
        let mut found = Vec::new();
        let chars: Vec<char> = arg.chars().collect();
        if chars[1] == '-' {
            for (at, opt) in self.opts.iter().enumerate() {
                for flag in &opt.flags {
                    if flag.starts_with(option_prefix) {
                        found.push((Some(at), flag.to_string(), sep.clone(), explicit.clone()));
                    }
                }
            }
        } else {
            let short_prefix: String = chars[..2].iter().collect();
            let short_explicit: String = chars[2..].iter().collect();
            for (at, opt) in self.opts.iter().enumerate() {
                for flag in &opt.flags {
                    if *flag == short_prefix {
                        found.push((Some(at), flag.to_string(), Some(String::new()), Some(short_explicit.clone())));
                    } else if flag.starts_with(option_prefix) {
                        found.push((Some(at), flag.to_string(), sep.clone(), explicit.clone()));
                    }
                }
            }
        }
        if !found.is_empty() {
            return Some(found);
        }
        if crate::re!(r"^(?:-\d+|-\d*\.\d+)\n?\z").is_match(arg) {
            return None;
        }
        if arg.contains(' ') {
            return None;
        }
        Some(vec![(None, arg.to_string(), None, None)])
    }

    /// `_get_values` for one option: its argument as its type, checked against its choices.
    fn value(&self, opt: &Opt, text: &str) -> Result<Val, String> {
        let invalid = |kind: &str| format!("argument {}: invalid {} value: {}", opt.name(), kind, base::str_repr(text));
        let value = match opt.kind {
            Kind::Int => Val::Int(base_int(text).ok_or_else(|| invalid("int"))?),
            Kind::Float => Val::Float(base_float(text).ok_or_else(|| invalid("float"))?),
            Kind::Path => Val::Str(base_path(text)),
            _ => Val::Str(text.to_string()),
        };
        if !opt.choices.is_empty() && !opt.choices.contains(&text) {
            let choices: Vec<String> = opt.choices.iter().map(|c| base::str_repr(c)).collect();
            return Err(format!("argument {}: invalid choice: {} (choose from {})", opt.name(), base::str_repr(text), choices.join(", ")));
        }
        Ok(value)
    }

    /// `parse_args(argv)`: each option's value by its `dest`.
    pub fn parse_args(&self, argv: &[String]) -> Result<IndexMap<String, Val>, Exit> {
        let mut values: IndexMap<String, Val> = self.opts.iter().filter(|o| o.kind != Kind::Help).map(|o| (o.dest(), o.default.clone())).collect();
        match self.parse_known_args(argv, &mut values) {
            Err(Stop::Error(message)) => Err(self.error(&message)),
            Err(Stop::Help) => Err(Exit { code: 0, stdout: self.format_help(), stderr: String::new() }),
            Ok(extras) if !extras.is_empty() => Err(self.error(&format!("unrecognized arguments: {}", extras.join(" ")))),
            Ok(_) => Ok(values),
        }
    }

    fn parse_known_args(&self, argv: &[String], values: &mut IndexMap<String, Val>) -> Result<Vec<String>, Stop> {
        // which words are options ('O'), which arguments ('A'), and the "--" ('-')
        let mut options: IndexMap<usize, Vec<(Option<usize>, String, Option<String>, Option<String>)>> = IndexMap::new();
        let mut pattern: Vec<char> = Vec::new();
        let mut plain = false;
        for (i, arg) in argv.iter().enumerate() {
            if plain {
                pattern.push('A');
            } else if arg == "--" {
                pattern.push('-');
                plain = true;
            } else {
                match self.parse_optional(arg) {
                    None => pattern.push('A'),
                    Some(tuples) => {
                        options.insert(i, tuples);
                        pattern.push('O');
                    }
                }
            }
        }
        let mut extras: Vec<String> = Vec::new();
        let max_option = options.keys().copied().max();
        let mut start = 0;
        while max_option.is_some_and(|m| start <= m) {
            // (no positionals: the words before the next option are extra)
            let next = (start..).find(|i| options.contains_key(i)).unwrap();
            extras.extend(argv[start..next].iter().cloned());
            start = self.consume_optional(next, argv, &pattern, &options[&next], values, &mut extras)?;
        }
        extras.extend(argv[start..].iter().cloned());
        Ok(extras)
    }

    #[allow(clippy::type_complexity)]
    fn consume_optional(&self, start_index: usize, argv: &[String], pattern: &[char], tuples: &[(Option<usize>, String, Option<String>, Option<String>)], values: &mut IndexMap<String, Val>, extras: &mut Vec<String>) -> Result<usize, Stop> {
        // if multiple actions match, the option string was ambiguous
        if tuples.len() > 1 {
            let options: Vec<&str> = tuples.iter().map(|t| t.1.as_str()).collect();
            return Err(Stop::Error(format!("ambiguous option: {} could match {}", argv[start_index], options.join(", "))));
        }
        let (mut action, mut option_string, mut sep, mut explicit_arg) = tuples[0].clone();
        let mut action_tuples: Vec<(usize, Option<String>)> = Vec::new();
        let stop;
        loop {
            // if we found no optional action, skip it
            let Some(at) = action else {
                extras.push(argv[start_index].clone());
                return Ok(start_index + 1);
            };
            let opt = &self.opts[at];
            match explicit_arg.clone() {
                Some(explicit) => {
                    let arg_count = if opt.takes_value() { 1 } else { 0 };
                    let single_dash = option_string.chars().nth(1) != Some('-');
                    if arg_count == 0 && single_dash && !explicit.is_empty() {
                        // a single-dash option that takes no arguments: more single-dash options out of the tail
                        if sep.as_deref().is_some_and(|s| !s.is_empty()) || explicit.starts_with('-') {
                            return Err(Stop::Error(format!("argument {}: ignored explicit argument {}", opt.name(), base::str_repr(&explicit))));
                        }
                        action_tuples.push((at, None));
                        let mut rest = explicit.chars();
                        let first = rest.next().unwrap();
                        option_string = format!("-{first}");
                        match self.flag(&option_string) {
                            Some(next) => {
                                action = Some(next);
                                let rest: String = rest.collect();
                                if rest.is_empty() {
                                    (sep, explicit_arg) = (None, None);
                                } else if let Some(after) = rest.strip_prefix('=') {
                                    (sep, explicit_arg) = (Some("=".to_string()), Some(after.to_string()));
                                } else {
                                    (sep, explicit_arg) = (Some(String::new()), Some(rest));
                                }
                            }
                            None => {
                                extras.push(format!("-{explicit}"));
                                stop = start_index + 1;
                                break;
                            }
                        }
                    } else if arg_count == 1 {
                        stop = start_index + 1;
                        action_tuples.push((at, Some(explicit)));
                        break;
                    } else {
                        return Err(Stop::Error(format!("argument {}: ignored explicit argument {}", opt.name(), base::str_repr(&explicit))));
                    }
                }
                None => {
                    let begin = start_index + 1;
                    if opt.takes_value() {
                        if pattern.get(begin) != Some(&'A') {
                            return Err(Stop::Error(format!("argument {}: expected one argument", opt.name())));
                        }
                        stop = begin + 1;
                        action_tuples.push((at, Some(argv[begin].clone())));
                    } else {
                        stop = begin;
                        action_tuples.push((at, None));
                    }
                    break;
                }
            }
        }
        for (at, text) in action_tuples {
            let opt = &self.opts[at];
            match (opt.kind, text) {
                (Kind::Help, _) => return Err(Stop::Help),
                (Kind::Flag, _) => {
                    values.insert(opt.dest(), Val::Bool(true));
                }
                (_, Some(text)) => {
                    let value = self.value(opt, &text).map_err(Stop::Error)?;
                    values.insert(opt.dest(), value);
                }
                (_, None) => unreachable!("an option that takes a value was given one"),
            }
        }
        Ok(stop)
    }
}

enum Stop {
    Error(String),
    Help,
}

fn is_py_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// Digits, with single underscores between them (the original's numbers): the digits alone.
fn digits(s: &str) -> Option<String> {
    let ok = !s.is_empty() && s.split('_').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    ok.then(|| s.replace('_', ""))
}

/// `int(text)` (None: its ValueError; also a number past 64 bits).
pub fn base_int(text: &str) -> Option<i64> {
    let s = text.trim_matches(is_py_space);
    let (negative, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let n: i64 = digits(body)?.parse().ok()?;
    Some(if negative { -n } else { n })
}

/// `float(text)` (None: its ValueError).
pub fn base_float(text: &str) -> Option<f64> {
    let s = text.trim_matches(is_py_space);
    let (negative, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let lower = body.to_ascii_lowercase();
    let x = if lower == "inf" || lower == "infinity" {
        f64::INFINITY
    } else if lower == "nan" {
        f64::NAN
    } else {
        // digits[.digits][e[+-]digits], digits on at least one side of the point
        let (mantissa, exponent) = match lower.split_once('e') {
            Some((m, e)) => (m, Some(e)),
            None => (lower.as_str(), None),
        };
        let (whole, fraction) = match mantissa.split_once('.') {
            Some((w, f)) => (w, Some(f)),
            None => (mantissa, None),
        };
        let whole_digits = if whole.is_empty() { Some(String::new()) } else { digits(whole) }?;
        let fraction_digits = match fraction {
            Some("") | None => String::new(),
            Some(f) => digits(f)?,
        };
        if whole_digits.is_empty() && fraction_digits.is_empty() {
            return None;
        }
        let mut plain = format!("{whole_digits}.{fraction_digits}0");
        if let Some(e) = exponent {
            let (sign, e) = match e.strip_prefix('-') {
                Some(rest) => ("-", rest),
                None => ("", e.strip_prefix('+').unwrap_or(e)),
            };
            plain.push_str(&format!("e{sign}{}", digits(e)?));
        }
        plain.parse::<f64>().ok()?
    };
    Some(if negative { -x } else { x })
}

/// `str(Path(text))`: doubled and trailing slashes and "." parts taken out.
pub fn base_path(text: &str) -> String {
    let parts: Vec<&str> = text.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    let root = if text.starts_with("//") && !text.starts_with("///") {
        "//"
    } else if text.starts_with('/') {
        "/"
    } else {
        ""
    };
    let joined = format!("{root}{}", parts.join("/"));
    if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

// ---------------------------------------------------------------------------------------------------------------------

/// Each game keeps its own history (what the bot learns of one game's cards says nothing of the other's).
/// (the original's default: `game="sts2"`.)
pub fn default_log_dir(game: &str) -> PathBuf {
    let base = supervisor::environ_or("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| supervisor::home().join(".local").join("state"));
    if game == "sts2" {
        base.join("sts2bot")
    } else {
        base.join("sts2bot").join(game)
    }
}

/// `sts2bot sts1-setup`'s parser.
pub fn sts1_setup_parser() -> Parser {
    let mut ap = Parser::new("sts2bot sts1-setup", "Point CommunicationMod (Slay the Spire) at the bot's relay.");
    ap.add(Opt::new(&["--port"], Kind::Int, Val::Int(games::default_port("sts1") as i64)));
    ap
}

/// `sts2bot sts1-setup`: have CommunicationMod start the relay the bot talks to.
pub fn sts1_setup(argv: &[String]) -> i32 {
    let ap = sts1_setup_parser();
    let args = match ap.parse_args(argv) {
        Ok(args) => args,
        Err(exit) => return exit.leave(),
    };
    let Val::Int(port) = args["port"] else { unreachable!("--port is an int") };
    let path = match sts1_host::write_comm_config(port, None) {
        Ok(path) => path,
        Err(e) => {
            // (the original left here with the OSError's traceback)
            eprintln!("{}", supervisor::os_error(&e, &sts1_host::comm_config_path().to_string_lossy()));
            return 1;
        }
    };
    println!("wrote {}", path.display());
    println!("CommunicationMod now starts the relay with the game. Start the bot with: sts2bot --game sts1");
    if sts1_host::mts_jar().is_none() {
        println!("ModTheSpire.jar was not found: subscribe to ModTheSpire, BaseMod and CommunicationMod on the Steam Workshop, or set STS1_MTS_JAR.");
    }
    0
}

/// The settings the command line gives (argparse's Namespace).
#[derive(Clone, Debug, PartialEq)]
pub struct Args {
    pub game: String,
    pub host: String,
    pub port: Option<i64>,
    pub character: String,
    pub runs: Option<i64>,
    pub first: Option<String>,
    pub no_tuning: bool,
    pub no_launch: bool,
    pub launch_cmd: Option<String>,
    pub fast_mode: String,
    pub full_graphics: bool,
    pub small_window: bool,
    pub windowed: bool,
    pub time_scale: f64,
    pub sts1_time_scale: f64,
    pub stuck_timeout: f64,
    pub poll: f64,
    pub log_dir: Option<PathBuf>,
    pub verbose: bool,
}

/// `sts2bot [options]`'s parser.
pub fn parser() -> Parser {
    let mut ap = Parser::new("sts2bot", "Plays Slay the Spire 2 nonstop through the STS2_MCP mod, the first Slay the Spire through CommunicationMod (--game sts1), or both at the same time (--game both).");
    let mut fast_modes: Vec<&'static str> = FAST_MODES.to_vec();
    fast_modes.push("keep");
    let g = |x: f64| base::float_g(x);
    ap.add(Opt::new(&["--game"], Kind::Str, Val::Str("sts2".into())).choices(&[games::GAMES[0], games::GAMES[1], "both"]).help(
        "which game to play: sts2 (default), sts1 (the first Slay the Spire), or both at once (a bot process for each, with the other options passed on to both)",
    ));
    ap.add(Opt::new(&["--host"], Kind::Str, Val::Str("127.0.0.1".into())));
    ap.add(Opt::new(&["--port"], Kind::Int, Val::None).help("where the bridge listens (default: 15526 for sts2, 15527 for sts1's relay)"));
    ap.add(Opt::new(&["--character"], Kind::Str, Val::Str("auto".into())).help(
        "\"auto\" and \"random\" play a character with the lowest ascension unlocked (drawn at random among those on that level), always at the highest ascension it has unlocked; or a character id (e.g. IRONCLAD; THE_SILENT, DEFECT or WATCHER in sts1), or \"cycle\" over unlocked ones (default: auto)",
    ));
    ap.add(Opt::new(&["--runs"], Kind::Int, Val::None).help("stop after this many runs (default: never stop)"));
    ap.add(Opt::new(&["--first"], Kind::Str, Val::None).metavar("CHARACTER").help("with --character auto: play this character until it has earned all of its own epochs, then carry on as usual"));
    ap.add(Opt::new(&["--no-tuning"], Kind::Flag, Val::Bool(false)).help("always play with the default settings instead of trying variations and learning from the results"));
    ap.add(Opt::new(&["--no-launch"], Kind::Flag, Val::Bool(false)).help("never start or kill the game; only play when it is up"));
    ap.add(Opt::new(&["--launch-cmd"], Kind::Str, Val::None).help(format!(
        "command that starts the game (default for sts2: {}; for sts1: ModTheSpire with BaseMod and CommunicationMod)",
        base::str_repr(&DEFAULT_LAUNCH())
    )));
    ap.add(Opt::new(&["--fast-mode"], Kind::Str, Val::Str("instant".into())).choices(&fast_modes).help(
        "game animation speed, set through the mod each time the bot connects; \"keep\" leaves the setting alone (default: instant, the fastest)",
    ));
    ap.add(Opt::new(&["--full-graphics"], Kind::Flag, Val::Bool(false)).help("keep the game's normal window and graphics (default: maximised window, no anti-aliasing, uncapped frame rate)"));
    ap.add(Opt::new(&["--small-window"], Kind::Flag, Val::Bool(false)).help("shrink the game window to 960x540 instead of full screen"));
    ap.add(Opt::new(&["--windowed"], Kind::Flag, Val::Bool(false)).help("a maximised window instead of full screen (both games)"));
    ap.add(Opt::new(&["--time-scale"], Kind::Float, Val::Float(DEFAULT_TIME_SCALE)).help(format!(
        "how many times faster the game's clock runs, set through the mod for the session (default: {}; 1 leaves it alone)",
        g(DEFAULT_TIME_SCALE)
    )));
    ap.add(Opt::new(&["--sts1-time-scale"], Kind::Float, Val::Float(DEFAULT_STS1_TIME_SCALE)).help(format!(
        "the same for the first Slay the Spire, through the bot's own ModTheSpire mod (default: {}; 1 leaves it alone)",
        g(DEFAULT_STS1_TIME_SCALE)
    )));
    ap.add(Opt::new(&["--stuck-timeout"], Kind::Float, Val::Float(300.0)).help("seconds without any change before the game is restarted"));
    ap.add(Opt::new(&["--poll"], Kind::Float, Val::Float(0.08)).help("seconds between state polls when there is nothing to do (0.2 until 2026-11-26)"));
    ap.add(Opt::new(&["--log-dir"], Kind::Path, Val::None).help("logs and run history (default: ~/.local/state/sts2bot, and its sts1/ for the first game)"));
    ap.add(Opt::new(&["-v", "--verbose"], Kind::Flag, Val::Bool(false)).help("log every action"));
    ap
}

/// `ap.parse_args(argv)` for `sts2bot [options]`.
pub fn parse_args(argv: &[String]) -> Result<Args, Exit> {
    let values = parser().parse_args(argv)?;
    let text = |k: &str| match &values[k] {
        Val::Str(s) => Some(s.clone()),
        _ => None,
    };
    let int = |k: &str| match values[k] {
        Val::Int(n) => Some(n),
        _ => None,
    };
    let float = |k: &str| match values[k] {
        Val::Float(x) => x,
        _ => unreachable!("{k} is a float"),
    };
    let flag = |k: &str| values[k] == Val::Bool(true);
    Ok(Args {
        game: text("game").unwrap_or_default(),
        host: text("host").unwrap_or_default(),
        port: int("port"),
        character: text("character").unwrap_or_default(),
        runs: int("runs"),
        first: text("first"),
        no_tuning: flag("no_tuning"),
        no_launch: flag("no_launch"),
        launch_cmd: text("launch_cmd"),
        fast_mode: text("fast_mode").unwrap_or_default(),
        full_graphics: flag("full_graphics"),
        small_window: flag("small_window"),
        windowed: flag("windowed"),
        time_scale: float("time_scale"),
        sts1_time_scale: float("sts1_time_scale"),
        stuck_timeout: float("stuck_timeout"),
        poll: float("poll"),
        log_dir: text("log_dir").map(PathBuf::from),
        verbose: flag("verbose"),
    })
}

/// `--game both`: a bot process for each game (games/both.rs), started and kept going from here.
/// (Err: the original's `ap.error`, which left with SystemExit.)
pub fn play_both(ap: &Parser, args: &Args, raw: &[String]) -> Result<i32, Exit> {
    let given = |opt: &str| match opt {
        "--port" => args.port.is_some(),
        "--log-dir" => args.log_dir.is_some(),
        _ => args.launch_cmd.is_some(),
    };
    let clash: Vec<&str> = both::GAME_OPTIONS.iter().copied().filter(|opt| given(opt)).collect();
    if !clash.is_empty() {
        return Err(ap.error(&format!("{} cannot be used with --game both: each game needs its own (run the two bots separately instead)", clash.join(", "))));
    }
    basic_config(Level::Info, vec![Handler { sink: Sink::Stderr, level: Level::Debug, format: "%(datetime)s both %(levelname)s %(message)s".to_string() }]);
    let why = sts1_host::comm_config_problem(games::default_port("sts1") as i64, None);
    if let Some(why) = why.filter(|w| !w.is_empty() && !args.no_launch) {
        log_error!(log, "{}. Neither game is started: the first game's bot {}", why, STS1_SETUP_HINT);
        return Ok(2);
    }
    let titles: Vec<&str> = games::GAMES.iter().map(|g| games::title(g)).collect();
    log_info!(log, "playing {}; logs in {} and {}", titles.join(" and "), default_log_dir(games::GAMES[0]).display(), default_log_dir(games::GAMES[1]).display());
    Ok(match HOOKS.with(|h| h.borrow().both_run.clone()) {
        Some(hook) => hook(raw),
        None if args.no_launch => both::run(raw, both::RunArgs { stop_games: Box::new(|_| Ok(())), ..Default::default() }), // (a low battery stops only the bots)
        None => both::run(raw, both::RunArgs::default()),
    })
}

/// `args`: the command line after the program's name (the original's `sys.argv[1:]`). Returns the exit code.
pub fn main(args: Vec<String>) -> i32 {
    match try_main(args) {
        Ok(code) => code,
        Err(exit) => exit.leave(),
    }
}

/// `main`, with what the parser had to say when it left by itself (the original's SystemExit) handed back instead of printed.
pub fn try_main(args: Vec<String>) -> Result<i32, Exit> {
    let raw = args;
    if raw.first().is_some_and(|w| w == "service") {
        // `sts2bot service disable` and friends: no game, no bot
        return Ok(service::main(&raw[1..], None));
    }
    if raw.first().is_some_and(|w| w == "sts1-setup") {
        return Ok(sts1_setup(&raw[1..]));
    }
    if raw.first().is_some_and(|w| w == "relay") {
        // what CommunicationMod starts with the game
        return Ok(relay::main(raw[1..].to_vec()));
    }
    let ap = parser();
    let mut args = parse_args(&raw)?;
    if args.game == "both" {
        return play_both(&ap, &args, &raw);
    }
    games::select(&args.game).expect("one of the parser's choices");
    learn::update_knobs(&learn::defaults(None)); // (with tuning, each run then draws its own from learn's choices)
    let port = args.port.filter(|p| *p != 0).unwrap_or(games::default_port(&args.game) as i64);
    args.port = Some(port);
    let Ok(port16) = u16::try_from(port) else {
        // (the original took any number and then never reached the game)
        return Err(ap.error(&format!("argument --port: no such port: {port}")));
    };
    let log_dir = args.log_dir.clone().filter(|d| !d.as_os_str().is_empty()).unwrap_or_else(|| default_log_dir(&args.game));
    args.log_dir = Some(log_dir.clone());

    std::fs::create_dir_all(&log_dir).unwrap_or_else(|e| panic!("{}", supervisor::os_error(&e, &log_dir.to_string_lossy())));
    // bot.log keeps its format (the analysis scripts read it); the terminal and the journal also say which game a line
    // is from (with --game both the two bots write to the same place).
    let screen = Handler { sink: Sink::Stderr, level: Level::Debug, format: format!("%(datetime)s {} %(levelname)s %(message)s", args.game) };
    // (A run takes about three minutes now: 5 MB held under half an hour of play, and the log reports read whole sessions.)
    let bot_log = log_dir.join("bot.log");
    let file = RotatingFile::new(&bot_log, 20_000_000, 8).unwrap_or_else(|e| panic!("{}", supervisor::os_error(&e, &bot_log.to_string_lossy())));
    let file = Handler { sink: Sink::File(file), level: Level::Debug, format: "%(datetime)s %(levelname)s %(message)s".to_string() };
    basic_config(if args.verbose { Level::Debug } else { Level::Info }, vec![screen, file]);

    let mut host: Option<Rc<dyn Host>> = None;
    let client: SharedGame;
    let launch_cmd;
    if args.game == "sts1" {
        if let Some(why) = sts1_host::comm_config_problem(port, None).filter(|w| !w.is_empty()) {
            if !args.no_launch {
                log_error!(log, "{}. Not launching the game: the bot {}", why, STS1_SETUP_HINT);
                return Ok(2);
            }
            log_warning!(log, "{}; the bot cannot reach the game until you run `sts2bot sts1-setup`", why);
        }
        client = Rc::new(RefCell::new(Sts1Client::new(&args.host, port16, 30.0, Prefs::Read(Box::new(|| sts1_host::read_prefs(None))))));
        host = Some(Rc::new(sts1_host::Sts1Host::new(Some(log_dir.clone()), !args.windowed, args.sts1_time_scale)));
        launch_cmd = args.launch_cmd.clone().filter(|c| !c.is_empty()).unwrap_or_else(sts1_host::default_launch);
    } else {
        client = Rc::new(RefCell::new(Client::new(&args.host, port16, 15.0)));
        launch_cmd = args.launch_cmd.clone().filter(|c| !c.is_empty()).unwrap_or_else(DEFAULT_LAUNCH);
    }
    let character = match &args.first {
        Some(first) if !first.is_empty() && args.character.to_lowercase() == "auto" => format!("auto:{first}"),
        _ => args.character.clone(),
    };
    let mut bot = Bot::new(Rc::clone(&client), BotArgs { character, log_dir: Some(log_dir.clone()), poll: args.poll, stuck_timeout: args.stuck_timeout, tune: !args.no_tuning, ..Default::default() });
    if args.game == "sts1" {
        (bot.room_pause, bot.room_poll) = (0.0, 0.03); // (see Bot::new: CommunicationMod already waits for the game)
    }
    let mut sup = Supervisor::new(
        bot,
        client,
        SupervisorArgs {
            host,
            launch_cmd,
            manage_game: !args.no_launch,
            fast_mode: if args.fast_mode == "keep" { None } else { Some(args.fast_mode.clone()) },
            light_graphics: !args.full_graphics,
            small_window: args.small_window,
            fullscreen: !args.windowed,
            time_scale: args.time_scale,
            ..Default::default()
        },
    );
    log_info!(log, "playing {}; logs and run history in {}", games::title(&args.game), log_dir.display());
    // (Ctrl+C ends the play; what it interrupted is not told apart)
    let _ = match HOOKS.with(|h| h.borrow().play.clone()) {
        Some(hook) => hook(&mut sup, args.runs),
        None => {
            let _ctrl_c = both::CtrlC::new();
            sup.run(args.runs)
        }
    };
    log_info!(log, "stopped after {} runs ({} wins)", sup.bot.runs_finished, sup.bot.wins);
    Ok(0)
}

#[cfg(test)]
mod tests {
    //! Against the original: tests/golden/supervisor/cli.json holds what the first version made of some two hundred
    //! command lines (the settings parsed, or the exit code and what was printed), of `service` command lines, and of
    //! whole `main` calls with the games' side faked (what was logged, what `both.run` and the supervisor were given).

    use super::*;
    use serde_json::{json, Map, Value};

    const GOLDEN: &str = include_str!("../tests/golden/supervisor/cli.json");

    fn golden() -> Value {
        serde_json::from_str(GOLDEN).unwrap()
    }

    fn words(v: &Value) -> Vec<String> {
        v.as_array().unwrap().iter().map(|w| w.as_str().unwrap().to_string()).collect()
    }

    fn float(x: f64) -> Value {
        json!({"f": base::float_repr(x)})
    }

    /// The seams as the golden cases were made: no home, Steam's launch command, a terminal `columns` wide.
    fn world(columns: &Value, more: &[(&str, String)]) -> supervisor::Patched {
        let mut env: IndexMap<String, String> = IndexMap::new();
        env.insert("HOME".to_string(), "/nonexistent-home".to_string());
        env.insert("PATH".to_string(), "/usr/bin:/bin".to_string());
        env.insert("COLUMNS".to_string(), columns.to_string());
        for (k, v) in more {
            env.insert(k.to_string(), v.clone());
        }
        supervisor::patch(|s| {
            s.environ = Some(env);
            s.DEFAULT_LAUNCH = Some(supervisor::STEAM_LAUNCH.to_string());
        })
    }

    fn exit_json(exit: &Exit) -> Value {
        json!({"code": exit.code, "stdout": exit.stdout, "stderr": exit.stderr})
    }

    #[test]
    fn the_command_line_is_read_as_the_record_read_it() {
        let golden = golden();
        let cases = golden["parse"].as_array().unwrap();
        assert!(cases.len() >= 170);
        for case in cases {
            let argv = words(&case["argv"]);
            let _world = world(&case["columns"], &[]);
            let got: Result<Value, Exit> = if argv.first().is_some_and(|w| w == "sts1-setup") {
                sts1_setup_parser().parse_args(&argv[1..]).map(|values| match values["port"] {
                    Val::Int(port) => json!({"port": port}),
                    _ => unreachable!(),
                })
            } else {
                parse_args(&argv).map(|a| {
                    json!({"game": a.game, "host": a.host, "port": a.port, "character": a.character, "runs": a.runs, "first": a.first, "no_tuning": a.no_tuning, "no_launch": a.no_launch,
                           "launch_cmd": a.launch_cmd, "fast_mode": a.fast_mode, "full_graphics": a.full_graphics, "small_window": a.small_window, "windowed": a.windowed,
                           "time_scale": float(a.time_scale), "sts1_time_scale": float(a.sts1_time_scale), "stuck_timeout": float(a.stuck_timeout), "poll": float(a.poll),
                           "log_dir": a.log_dir.map(|d| d.to_string_lossy().into_owned()), "verbose": a.verbose})
                })
            };
            match got {
                Ok(parsed) => assert_eq!(parsed, case["parsed"], "{argv:?}"),
                Err(exit) => assert_eq!(exit_json(&exit), case["exit"], "{argv:?} at {} columns", case["columns"]),
            }
        }
    }

    #[test]
    fn service_commands_go_to_the_service() {
        let golden = golden();
        let cases = golden["service"].as_array().unwrap();
        assert!(cases.len() >= 27);
        for case in cases {
            let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
            let (seen, rc) = (Rc::clone(&calls), case["rc"].clone());
            let _run = service::patch_run(move |args| {
                seen.borrow_mut().push(args.to_vec());
                match rc.as_i64() {
                    Some(code) => Ok(code as i32),
                    None => Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
                }
            });
            let (code, printed) = service::capture_print(|| try_main(words(&case["argv"])).unwrap());
            let got = json!({"argv": case["argv"], "rc": case["rc"], "code": code, "calls": *calls.borrow(), "stdout": printed.stdout, "stderr": printed.stderr});
            assert_eq!(&got, case);
        }
    }

    #[test]
    fn main_starts_what_the_record_started() {
        let golden = golden();
        let cases = golden["main"].as_array().unwrap();
        assert!(cases.len() >= 31);
        let relay = format!("{} relay", sts1_host::executable());
        for case in cases {
            let tmp = tempfile::tempdir().unwrap();
            let t = tmp.path().canonicalize().unwrap().to_string_lossy().into_owned();
            let _world = world(&case["columns"], &[("XDG_CONFIG_HOME", format!("{t}/cfg")), ("XDG_STATE_HOME", format!("{t}/state"))]);
            let _game = games::playing("sts2");
            let _knobs = learn::with_knobs(&[]);
            let both_calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
            let played: Rc<RefCell<Vec<Value>>> = Rc::default();
            let (seen_both, seen_play) = (Rc::clone(&both_calls), Rc::clone(&played));
            let _hooks = patch(|h| {
                h.basic_config = Some(Rc::new(|_, _| {}));
                h.both_run = Some(Rc::new(move |raw| {
                    seen_both.borrow_mut().push(raw.to_vec());
                    0
                }));
                h.play = Some(Rc::new(move |sup, max_runs| {
                    let b = &sup.bot;
                    seen_play.borrow_mut().push(json!({"runs": max_runs, "launch_cmd": sup.launch_cmd, "manage_game": sup.manage_game, "fast_mode": sup.fast_mode, "light_graphics": sup.light_graphics,
                        "small_window": sup.small_window, "fullscreen": sup.fullscreen, "time_scale": base::float_repr(sup.time_scale), "boot_timeout": base::float_repr(sup.boot_timeout),
                        "outage_grace": base::float_repr(sup.outage_grace), "heal_mod": sup.heal_mod, "host": sup.host.name(), "character": b.character, "poll": base::float_repr(b.poll),
                        "stuck_timeout": base::float_repr(b.stuck_timeout), "tune": b.tune, "room_pause": base::float_repr(b.room_pause), "room_poll": base::float_repr(b.room_poll),
                        "log_dir": b.log_dir.as_ref().map(|d| d.to_string_lossy().into_owned()), "game": games::active()}));
                    Ok(())
                }));
            });
            if let Some(config) = case["config"].as_str() {
                match config.strip_prefix("ok:") {
                    Some(port) => {
                        sts1_host::write_comm_config(port.parse().unwrap(), None).unwrap();
                    }
                    None => {
                        std::fs::create_dir_all(sts1_host::comm_config_path().parent().unwrap()).unwrap();
                        std::fs::write(sts1_host::comm_config_path(), config.replace("{RELAY}", &relay)).unwrap();
                    }
                }
            }
            let mut argv = words(&case["argv"]);
            if case["log_dir"] == true {
                argv.extend(["--log-dir".to_string(), format!("{t}/logs")]);
            }
            let (ended, records) = logging::capture(|| try_main(argv));
            let logs: Vec<Value> = records.iter().filter(|r| r.logger == "sts2bot").map(|r| json!([r.level.name(), r.message])).collect();
            let mut got: Map<String, Value> = Map::new();
            for key in ["argv", "config", "log_dir", "columns"] {
                got.insert(key.to_string(), case[key].clone());
            }
            match ended {
                Ok(code) => got.insert("code".to_string(), json!(code)),
                Err(exit) => got.insert("exit".to_string(), exit_json(&exit)),
            };
            got.insert("logs".to_string(), json!(logs));
            got.insert("both".to_string(), json!(*both_calls.borrow()));
            got.insert("played".to_string(), json!(*played.borrow()));
            let got: Value = serde_json::from_str(&Value::Object(got).to_string().replace(&t, "<T>").replace(&relay, "{RELAY}")).unwrap();
            // (which address the client was given cannot be read back through the shared game: left out here)
            let mut want = case.clone();
            for run in want["played"].as_array_mut().unwrap() {
                run.as_object_mut().unwrap().shift_remove("base");
            }
            assert_eq!(got, want, "{}", case["argv"]);
        }
    }

    #[test]
    fn numbers_and_paths_as_the_record_reads_them() {
        assert_eq!(base_int(" +1_0 "), Some(10));
        assert_eq!(base_int("1__0"), None);
        assert_eq!(base_float("-.5e-3"), Some(-0.0005));
        assert!(base_float("NaN").unwrap().is_nan());
        assert_eq!(base_path("a//b/./c/"), "a/b/c");
        assert_eq!(base_path(""), ".");
        assert_eq!(base_path("//x"), "//x");
        assert_eq!(base_path("///x"), "/x");
    }
}
