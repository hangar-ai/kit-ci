//! Shared by the tests: recorded `gh` output, and a fake Hangar that answers
//! like the real one and keeps a clock the tests move by hand.
//!
//! `Fake` answers the calls of the tab as the harness holds them, and its
//! `advance` fires the tab's timers when their time comes. The recorded output
//! of `gh` is in `test/fixtures/`, which `test/native.test.js` reads too.

#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap, HashSet};

use hangar_kit_sdk::api::KitError;
use hangar_kit_sdk::format::parse_iso;
use hangar_kit_sdk::testing::{self, Harness, StartExt};
use hangar_kit_sdk::ui::abi::Call;
use hangar_kit_sdk::ui::{
    Banner, Bar, Button, Element, Empty, Glyph, HandlerId, Header, ListItem, Log, Symbol,
};
use kit_ci_tab::Ci;
use serde_json::{Value, json};

pub const RUN_SUCCESS: &str = include_str!("../../../test/fixtures/run-success.json");
pub const RUN_FAILURE: &str = include_str!("../../../test/fixtures/run-failure.json");
pub const RUN_RUNNING: &str = include_str!("../../../test/fixtures/run-running.json");
pub const RUN_LIST: &str = include_str!("../../../test/fixtures/run-list.json");
pub const LOG_FAILED: &str = include_str!("../../../test/fixtures/log-failed.txt");
pub const NOT_LOGGED_IN: &str = include_str!("../../../test/fixtures/not-logged-in.stderr");

pub const ID: &str = "41222364482";
pub const PERMISSIONS: [&str; 4] = [
    "command:ci-run",
    "command:ci-list",
    "command:ci-log",
    "notify",
];

/// A moment, in ms since the epoch.
pub fn at(iso: &str) -> i64 {
    parse_iso(iso).expect("a date")
}

/// The test clock's start: `Date.parse("2026-10-31T17:02:00Z")`.
pub fn start_of_day() -> i64 {
    at("2026-10-31T17:02:00Z")
}

/// What a command does when the tab calls it.
pub enum Said {
    /// It ran: its exit code (none when it did not end), output and flags.
    Ran {
        exit_code: Option<i32>,
        stdout: String,
        stderr: String,
        timed_out: bool,
        truncated: bool,
    },
    /// Hangar refuses the call, with this code (`not_found`, `denied`…).
    Refused(&'static str),
    /// No answer yet: the test gives it later with `release`.
    Held,
}

/// `exitCode: 0`, and this output.
pub fn ok(stdout: impl Into<String>) -> Said {
    Said::Ran {
        exit_code: Some(0),
        stdout: stdout.into(),
        stderr: String::new(),
        timed_out: false,
        truncated: false,
    }
}

/// A command that ends with this code and this on its stderr.
pub fn fails(exit_code: i32, stderr: impl Into<String>) -> Said {
    Said::Ran {
        exit_code: Some(exit_code),
        stdout: String::new(),
        stderr: stderr.into(),
        timed_out: false,
        truncated: false,
    }
}

/// A command Hangar cut off at its own limit.
pub fn timed_out() -> Said {
    Said::Ran {
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        timed_out: true,
        truncated: false,
    }
}

/// A command whose output was cut at its limit.
pub fn cut(stdout: impl Into<String>) -> Said {
    Said::Ran {
        exit_code: Some(0),
        stdout: stdout.into(),
        stderr: String::new(),
        timed_out: false,
        truncated: true,
    }
}

type Answerer = Box<dyn FnMut(&BTreeMap<String, String>) -> Said>;

/// What the tab asks of Hangar, answered the way Hangar does.
pub struct Setup {
    now: i64,
    params: BTreeMap<String, String>,
    locale: String,
    commands: HashMap<String, Answerer>,
    deny_notify: bool,
    grant: Vec<String>,
}

impl Setup {
    /// A tab opened for run `ID`, in English, with every permission.
    pub fn new() -> Self {
        Setup {
            now: start_of_day(),
            params: BTreeMap::from([("id".to_owned(), ID.to_owned())]),
            locale: "en".to_owned(),
            commands: HashMap::new(),
            deny_notify: false,
            grant: PERMISSIONS.iter().map(|p| (*p).to_owned()).collect(),
        }
    }

    /// The clock starts here (ISO 8601).
    pub fn now(mut self, iso: &str) -> Self {
        self.now = at(iso);
        self
    }

    pub fn params(mut self, params: &[(&str, &str)]) -> Self {
        self.params = params
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        self
    }

    pub fn lang(mut self, locale: &str) -> Self {
        self.locale = locale.to_owned();
        self
    }

    pub fn deny_notify(mut self) -> Self {
        self.deny_notify = true;
        self
    }

    /// How the declared command `name` answers, each time it is called.
    pub fn command(
        mut self,
        name: &str,
        answer: impl FnMut(&BTreeMap<String, String>) -> Said + 'static,
    ) -> Self {
        self.commands.insert(name.to_owned(), Box::new(answer));
        self
    }

    /// A command that always answers `said`'s output.
    pub fn serves(self, name: &str, stdout: &'static str) -> Self {
        self.command(name, move |_| ok(stdout))
    }

    pub fn start(self) -> Fake {
        testing::set_now(Some(self.now));
        let mut start = testing::start()
            .grant(self.grant.iter().map(String::as_str))
            .locale(&self.locale);
        for (name, value) in &self.params {
            start = start.param(name, value);
        }
        let tab = Harness::<Ci>::new(start);
        let mut fake = Fake {
            tab,
            commands: self.commands,
            deny_notify: self.deny_notify,
            history: Vec::new(),
            seen: HashSet::new(),
            held: Vec::new(),
            clock: self.now,
            armed: HashMap::new(),
        };
        fake.pump();
        fake
    }
}

struct Armed {
    due: i64,
    every: i64,
    once: bool,
}

/// The tab, started, with a Hangar answering it.
pub struct Fake {
    pub tab: Harness<Ci>,
    commands: HashMap<String, Answerer>,
    deny_notify: bool,
    /// Every call the tab made, in order.
    pub history: Vec<Call>,
    seen: HashSet<u64>,
    /// Calls to commands the test holds back: id, command.
    held: Vec<(u64, String)>,
    clock: i64,
    armed: HashMap<String, Armed>,
}

fn answer_json(said: &Said) -> Result<Value, KitError> {
    match said {
        Said::Ran {
            exit_code,
            stdout,
            stderr,
            timed_out,
            truncated,
        } => Ok(json!({
            "exitCode": exit_code,
            "stdout": stdout,
            "stderr": stderr,
            "timedOut": timed_out,
            "truncated": truncated,
        })),
        Said::Refused(code) => Err(KitError::new(*code, format!("refused: {code}"))),
        Said::Held => unreachable!("a held call is not answered"),
    }
}

impl Fake {
    /// Answers every call that is waiting, the way Hangar would; the commands
    /// as the setup says, the rest as they are. Calls to a held command stay.
    pub fn pump(&mut self) {
        while let Some(call) = self
            .tab
            .calls()
            .iter()
            .find(|call| !self.held.iter().any(|(id, _)| *id == call.id))
            .cloned()
        {
            if self.seen.insert(call.id) {
                self.history.push(call.clone());
            }
            let answer = match call.method.as_str() {
                "command.run" => {
                    let name = call.params["name"].as_str().unwrap_or_default().to_owned();
                    let args: BTreeMap<String, String> = call.params["args"]
                        .as_object()
                        .map(|args| {
                            args.iter()
                                .map(|(k, v)| {
                                    (k.clone(), v.as_str().unwrap_or_default().to_owned())
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    match self.commands.get_mut(&name) {
                        None => Err(KitError::new(
                            KitError::DENIED,
                            "this tab was not granted it",
                        )),
                        Some(answer) => match answer(&args) {
                            Said::Held => {
                                self.held.push((call.id, name));
                                continue;
                            }
                            said => answer_json(&said),
                        },
                    }
                }
                "tab.setTitle" | "tab.setBadge" => Ok(json!({})),
                "notify" if self.deny_notify => Err(KitError::new(KitError::DENIED, "not granted")),
                "notify" => Ok(Value::Null),
                "ui.toast" => Ok(json!({ "action": false })),
                "ui.copy" => Ok(json!({})),
                other => Err(KitError::new(KitError::UNKNOWN, format!("unknown {other}"))),
            };
            self.tab.reply(call.id, answer);
        }
        self.arm();
    }

    /// Gives the held call to `command` this answer, and answers what that
    /// brings.
    pub fn release(&mut self, command: &str, said: Said) {
        let at = self
            .held
            .iter()
            .position(|(_, name)| name == command)
            .unwrap_or_else(|| panic!("no call to {command} is held"));
        let (id, _) = self.held.remove(at);
        self.tab.reply(id, answer_json(&said));
        self.pump();
    }

    /// Whether a call to `command` is held.
    pub fn holds(&self, command: &str) -> bool {
        self.held.iter().any(|(_, name)| name == command)
    }

    /// The tab's timers, as the harness says them, kept by their due time.
    fn arm(&mut self) {
        let now = self.clock;
        let subscribed: Vec<_> = self.tab.subscriptions().timers.clone();
        self.armed
            .retain(|id, _| subscribed.iter().any(|timer| &timer.id == id));
        for timer in subscribed {
            self.armed.entry(timer.id.clone()).or_insert(Armed {
                due: now + timer.every_ms as i64,
                every: timer.every_ms as i64,
                once: timer.id.starts_with("~after:"),
            });
        }
    }

    fn set_clock(&mut self, ms: i64) {
        self.clock = ms;
        testing::set_now(Some(ms));
    }

    /// Moves the clock, firing the tab's timers in order as their time comes,
    /// answering what each brings before the next.
    pub fn advance(&mut self, ms: i64) {
        let end = self.clock + ms;
        loop {
            self.pump();
            let due = self
                .armed
                .iter()
                .filter(|(_, timer)| timer.due <= end)
                .min_by_key(|(id, timer)| (timer.due, (*id).clone()))
                .map(|(id, timer)| (id.clone(), timer.due, timer.every, timer.once));
            let Some((id, due, every, once)) = due else {
                break;
            };
            self.set_clock(self.clock.max(due));
            if once {
                self.armed.remove(&id);
            } else if let Some(timer) = self.armed.get_mut(&id) {
                timer.due += every;
            }
            self.tab.timer(&id);
        }
        self.set_clock(end);
        self.pump();
    }

    /// Presses a button, a link or a row, and answers what that brings.
    pub fn press(&mut self, label: &str) {
        self.tab.press(label);
        self.pump();
    }

    pub fn event(&mut self, name: &str, data: Value) {
        self.tab.event(name, data);
        self.pump();
    }

    /// A new language: Hangar says so with the `theme` event.
    pub fn speak(&mut self, locale: &str) {
        self.event("theme", json!({ "scheme": "dark", "locale": locale }));
    }

    /// The calls of one method that the tab made, in order.
    pub fn calls_to(&self, method: &str) -> Vec<&Call> {
        self.history
            .iter()
            .filter(|call| call.method == method)
            .collect()
    }

    /// The calls to one declared command.
    pub fn runs_of(&self, command: &str) -> Vec<&Call> {
        self.calls_to("command.run")
            .into_iter()
            .filter(|call| call.params["name"] == command)
            .collect()
    }

    /// The states `tab.setBadge` was told, in order (null as `Null`).
    pub fn badges(&self) -> Vec<Value> {
        self.calls_to("tab.setBadge")
            .iter()
            .map(|call| call.params["state"].clone())
            .collect()
    }

    pub fn titles(&self) -> Vec<String> {
        self.calls_to("tab.setTitle")
            .iter()
            .map(|call| call.params["title"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    /// Whether the tab waits for its next poll.
    pub fn polls(&self) -> bool {
        self.tab.waits("poll")
    }

    /// Whether the tab asks to be woken by a clock of this id.
    pub fn ticks(&self, id: &str) -> bool {
        self.tab.subscriptions().timers.iter().any(|t| t.id == id)
    }

    // ---- The page.

    /// Every widget of the page, parents before children.
    pub fn widgets(&self) -> Vec<Element<HandlerId>> {
        let mut found = Vec::new();
        self.tab
            .view()
            .root
            .for_each(&mut |element| found.push(element.clone()));
        found
    }

    pub fn header(&self) -> Header<HandlerId> {
        self.widgets()
            .into_iter()
            .find_map(|e| match e {
                Element::Header(header) => Some(header),
                _ => None,
            })
            .expect("a header")
    }

    /// The rows of every list on the page, in order.
    pub fn items(&self) -> Vec<ListItem<HandlerId>> {
        self.widgets()
            .into_iter()
            .flat_map(|e| match e {
                Element::List(list) => list.items,
                _ => Vec::new(),
            })
            .collect()
    }

    /// The glyphs of the page, those in a row's lead included.
    pub fn glyphs(&self) -> Vec<Glyph> {
        self.widgets()
            .into_iter()
            .filter_map(|e| match e {
                Element::Glyph(glyph) => Some(glyph),
                _ => None,
            })
            .collect()
    }

    /// The titles and counts of the sections, in order.
    pub fn sections(&self) -> Vec<(String, Option<String>)> {
        self.widgets()
            .into_iter()
            .filter_map(|e| match e {
                Element::Section(section) => Some((section.title, section.count)),
                _ => None,
            })
            .collect()
    }

    pub fn banners(&self) -> Vec<Banner<HandlerId>> {
        self.widgets()
            .into_iter()
            .filter_map(|e| match e {
                Element::Banner(banner) => Some(banner),
                _ => None,
            })
            .collect()
    }

    pub fn empties(&self) -> Vec<Empty<HandlerId>> {
        self.widgets()
            .into_iter()
            .filter_map(|e| match e {
                Element::Empty(empty) => Some(empty),
                _ => None,
            })
            .collect()
    }

    pub fn buttons(&self) -> Vec<Button<HandlerId>> {
        self.widgets()
            .into_iter()
            .filter_map(|e| match e {
                Element::Button(button) => Some(button),
                _ => None,
            })
            .collect()
    }

    pub fn log(&self) -> Option<Log<HandlerId>> {
        self.widgets().into_iter().find_map(|e| match e {
            Element::Log(log) => Some(log),
            _ => None,
        })
    }

    /// The bar at the foot of the page.
    pub fn foot(&self) -> Option<Bar<HandlerId>> {
        self.widgets().into_iter().find_map(|e| match e {
            Element::Bar(bar) if bar.foot => Some(bar),
            _ => None,
        })
    }

    pub fn icons(&self) -> Vec<Symbol> {
        self.widgets()
            .into_iter()
            .filter_map(|e| match e {
                Element::Icon(icon) => Some(icon),
                _ => None,
            })
            .collect()
    }
}

/// What a widget says, as text.
pub fn said(element: &Element<HandlerId>) -> String {
    match element {
        Element::Text(text) => text.text.clone(),
        Element::State(state) => state.label.clone(),
        Element::Glyph(glyph) => glyph.label.clone(),
        Element::Pre(pre) => pre.text.clone(),
        Element::Row(row) => row.children.iter().map(said).collect::<Vec<_>>().join(" "),
        Element::Button(button) => button.label.clone().unwrap_or_default(),
        _ => String::new(),
    }
}
