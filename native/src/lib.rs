//! CI status, the native tab: follows a GitHub Actions run, or lists the
//! recent ones to choose from.
//!
//! Rust built to WebAssembly: `tabs/ci.wasm` is what Hangar runs. It calls
//! these host methods, under the permissions the manifest declares:
//!
//! - `command.run` of `ci-run`, `ci-list` and `ci-log`, which are `gh`;
//! - `tab.setTitle` and `tab.setBadge`, set on load and again when they change;
//! - `notify`, once, when a run it followed ends.
//!
//! Where the files are:
//!
//! - `logic.rs`: gh's JSON and logs turned into what to show;
//! - `strings.rs` holds the words, English and French;
//! - this file: state, `update`, the 5 s poll, the clock;
//! - `view.rs`: the state drawn as widgets.
//!
//! A refusal or a failure is a state of the tab, drawn, never a panic.

use std::collections::BTreeMap;
use std::time::Duration;

use hangar_kit_sdk::api::{CommandAnswer, TabParamsAnswer, Theme};
use hangar_kit_sdk::prelude::*;

pub mod logic;
pub mod strings;
mod view;

use logic::{ErrorKind, Fault, Kind, Log, Run, RunRow};

/// A followed run is read again this often until it ends.
pub const POLL: Duration = Duration::from_secs(5);
/// The clock of the jobs and of the run's time, while a run is going.
pub const TICK: Duration = Duration::from_secs(1);
/// Ages ("2 min ago") keep moving on a page nothing else redraws.
pub const AGE: Duration = Duration::from_secs(60);
/// Failures in a row, while following a run, before the vigil ends.
pub const MAX_FAILURES: u32 = 3;
/// The lines of a log given to Hangar at once, and at most when it asks: Hangar asks
/// for about 440, and a view may weigh 4 MB, which a thousand lines of the longest
/// a log may have (`LINE_MAX`) stay well under.
const WINDOW: usize = 400;
const WINDOW_MAX: usize = 1000;

/// What the tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Loading,
    Picker,
    Run,
    Error,
}

/// Why nothing is shown: what went wrong, and what `gh` said when it did not
/// explain itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub kind: ErrorKind,
    pub detail: String,
}

/// The log of the failed steps, once it came.
#[derive(Debug, Clone, PartialEq)]
pub enum LogState {
    /// Nothing to show: the command answered nothing.
    Empty,
    /// The command failed; the run's result stands.
    Failed(ErrorKind),
    Lines(Box<Lines>),
}

/// A log, and where the person is in it. Hangar draws only the lines in view
/// and asks for others as they scroll, so a view never carries the whole of
/// a megabyte.
#[derive(Debug, Clone, PartialEq)]
pub struct Lines {
    pub log: Log,
    /// The error being looked at, as an index into `log.errors`.
    pub current: usize,
    /// The lines given to Hangar: from, and to before.
    pub window: (usize, usize),
}

impl Lines {
    fn new(log: Log) -> Self {
        let total = log.lines.len();
        // It opens on its first error, with a few lines before it, or at its
        // end when there is none.
        let window = match log.errors.first() {
            Some(&first) => around(first, total),
            None => (total.saturating_sub(WINDOW), total),
        };
        Lines {
            log,
            current: 0,
            window,
        }
    }

    /// The line being looked at, if the log has errors.
    pub fn current_line(&self) -> Option<usize> {
        self.log.errors.get(self.current).copied()
    }
}

/// `WINDOW` lines with `line` a quarter of the way down.
fn around(line: usize, total: usize) -> (usize, usize) {
    let start = line.saturating_sub(WINDOW / 4).min(total);
    (start, (start + WINDOW).min(total))
}

pub struct Ci {
    pub t: Strings,
    /// The clock, as of the last update: the view is a function of the state.
    pub now: i64,
    pub utc_offset: i32,
    pub visible: bool,
    pub screen: Screen,
    /// The run being followed; none in the picker.
    pub id: Option<String>,
    /// The run was chosen in the picker, which it goes back to.
    pub from_picker: bool,
    pub run: Option<Run>,
    /// None while it is being read.
    pub log: Option<LogState>,
    pub runs: Option<Vec<RunRow>>,
    /// The picker shows what it last knew while it asks again.
    pub refreshing: bool,
    pub error: Option<Problem>,
    /// A refresh failed while following a run, and will be tried again.
    pub warning: bool,
    /// A later request makes the earlier answers stale.
    pub generation: u32,
    saw_running: bool,
    failures: u32,
    last_title: Option<String>,
    last_badge: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Message {
    /// A run chosen in the picker.
    Open(String),
    Back,
    /// Tries again what failed.
    Retry,
    /// The refresh button of the picker.
    Refresh,
    Run(u32, Result<CommandAnswer, KitError>),
    List(u32, Result<CommandAnswer, KitError>),
    Log(u32, Result<CommandAnswer, KitError>),
    /// Time to read the run again.
    Poll(u32),
    /// The clock moved.
    Tick,
    Visible(bool),
    Theme(Theme),
    Params(TabParamsAnswer),
    PreviousError,
    NextError,
    /// Hangar wants other lines of the log.
    Lines(Range),
    Copy(String),
    Copied(bool),
}

/// What a command made of a call: its output, or why it gave none.
type Ran = Result<(String, bool), Problem>;

/// `command.run` answered: its stdout when it succeeded, else why not.
fn outcome(answer: Result<CommandAnswer, KitError>) -> Ran {
    match answer {
        Err(error) => Err(Problem {
            kind: logic::classify(Fault::Rejected { code: &error.code }),
            detail: error.message,
        }),
        Ok(answer) if answer.exit_code == Some(0) => Ok((answer.stdout, answer.truncated)),
        Ok(answer) => {
            let fault = Fault::Ended {
                exit_code: answer.exit_code,
                stderr: &answer.stderr,
                timed_out: answer.timed_out,
            };
            let said = if answer.stderr.is_empty() {
                &answer.stdout
            } else {
                &answer.stderr
            };
            Err(Problem {
                kind: logic::classify(fault),
                detail: logic::strip_ansi(said).trim().chars().take(500).collect(),
            })
        }
    }
}

fn command(name: &str, id: Option<&str>) -> api::CommandRun {
    api::CommandRun {
        name: name.to_owned(),
        args: id
            .map(|id| BTreeMap::from([("id".to_owned(), id.to_owned())]))
            .unwrap_or_default(),
    }
}

impl Ci {
    /// What a first read, a refresh and a reload all come to: a run is
    /// readable, or the person is asked to try again.
    fn busy(&self) -> bool {
        self.refreshing || self.screen == Screen::Loading
    }

    /// Every request is the latest: what the earlier ones answer is dropped.
    fn reset(&mut self, cx: &mut Context<Message>) {
        self.generation = self.generation.wrapping_add(1);
        cx.cancel("poll");
        self.saw_running = false;
        self.failures = 0;
        self.run = None;
        self.log = None;
        self.error = None;
        self.warning = false;
        self.refreshing = false;
    }

    // Title and badge are the tab's own business and never worth failing for.
    fn set_title(&mut self, title: String, cx: &mut Context<Message>) {
        if self.last_title.as_ref() == Some(&title) {
            return;
        }
        self.last_title = Some(title.clone());
        cx.call_quietly(api::TabSetTitle { title });
    }

    fn set_badge(&mut self, badge: Option<logic::Badge>, cx: &mut Context<Message>) {
        let (state, text) = match badge {
            Some(badge) => (Some(badge.state), Some(self.t.t(badge.key))),
            None => (None, None),
        };
        let key = match &text {
            Some(text) => format!("{state:?}|{text}"),
            None => "none".to_owned(),
        };
        if self.last_badge.as_ref() == Some(&key) {
            return;
        }
        self.last_badge = Some(key);
        cx.call_quietly(api::TabSetBadge { state, text });
    }

    fn notify_end(&self, view: &logic::RunView, cx: &mut Context<Message>) {
        let kind = if view.kind == Kind::Queued {
            Kind::Neutral
        } else {
            view.kind
        };
        let name = if view.name.is_empty() {
            self.t.t("tab")
        } else {
            view.name.clone()
        };
        let state = self.t.t(kind.key()).to_lowercase();
        let body: String = [view.title.as_str(), view.branch.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
            .chars()
            .take(300)
            .collect();
        // Not granted, or over the rate limit: the badge says it too.
        cx.call_quietly(api::Notify {
            title: self
                .t
                .tv("notifyTitle", &[("name", &name), ("state", &state)]),
            body: (!body.is_empty()).then_some(body),
        });
    }

    fn poll(&self, cx: &mut Context<Message>) {
        cx.after("poll", POLL, Message::Poll(self.generation));
    }

    fn fetch_run(&self, id: &str, cx: &mut Context<Message>) {
        let generation = self.generation;
        cx.call(command("ci-run", Some(id)), move |answer| {
            Message::Run(generation, answer)
        });
    }

    fn fetch_list(&self, cx: &mut Context<Message>) {
        let generation = self.generation;
        cx.call(command("ci-list", None), move |answer| {
            Message::List(generation, answer)
        });
    }

    fn load_log(&self, id: &str, cx: &mut Context<Message>) {
        let generation = self.generation;
        cx.call(command("ci-log", Some(id)), move |answer| {
            Message::Log(generation, answer)
        });
    }

    fn open_run(&mut self, id: String, from_picker: bool, cx: &mut Context<Message>) {
        self.reset(cx);
        self.screen = Screen::Loading;
        self.id = Some(id.clone());
        self.from_picker = from_picker;
        self.fetch_run(&id, cx);
    }

    fn show_picker(&mut self, cx: &mut Context<Message>) {
        self.reset(cx);
        let known = self.runs.is_some();
        self.screen = if known {
            Screen::Picker
        } else {
            Screen::Loading
        };
        self.id = None;
        self.from_picker = false;
        self.refreshing = known;
        self.set_title(self.t.t("tab"), cx);
        self.set_badge(None, cx);
        self.fetch_list(cx);
    }

    fn handle_params(&mut self, params: &BTreeMap<String, String>, cx: &mut Context<Message>) {
        match params.get("id") {
            Some(id) if logic::is_run_id(id) => self.open_run(id.clone(), false, cx),
            _ => self.show_picker(cx),
        }
    }

    /// Tries again what failed, or refreshes the picker.
    fn retry(&mut self, cx: &mut Context<Message>) {
        match self.id.clone() {
            Some(id) => self.open_run(id, self.from_picker, cx),
            None => self.show_picker(cx),
        }
    }

    fn run_answered(&mut self, answer: Result<CommandAnswer, KitError>, cx: &mut Context<Message>) {
        let Some(id) = self.id.clone() else { return };
        let problem = match outcome(answer) {
            Ok((stdout, _)) => match Run::parse(&stdout) {
                Some(run) => return self.run_read(id, run, cx),
                None => Problem {
                    kind: ErrorKind::Error,
                    detail: stdout.chars().take(300).collect(),
                },
            },
            Err(problem) => problem,
        };

        // Failed. A transient failure while following a run is retried; the
        // rest is shown.
        self.failures += 1;
        if self.run.is_some() && !problem.kind.is_fatal() && self.failures < MAX_FAILURES {
            self.warning = true;
            self.poll(cx);
            return;
        }
        cx.cancel("poll");
        self.screen = Screen::Error;
        self.error = Some(problem);
        self.warning = false;
        self.set_badge(None, cx);
    }

    fn run_read(&mut self, id: String, run: Run, cx: &mut Context<Message>) {
        self.failures = 0;
        self.warning = false;
        self.error = None;
        self.screen = Screen::Run;
        let view = logic::run_view(&run, self.now);
        let badge = logic::badge_for(&run);
        self.run = Some(run);
        let title = if view.name.is_empty() {
            self.t.tv("run", &[("id", &id)])
        } else {
            view.name.clone()
        };
        self.set_title(title, cx);
        self.set_badge(Some(badge), cx);
        if view.done {
            cx.cancel("poll");
            let ended = std::mem::take(&mut self.saw_running);
            if ended {
                self.notify_end(&view, cx);
            }
            if view.kind == Kind::Failure {
                self.load_log(&id, cx);
            }
        } else {
            self.saw_running = true;
            self.poll(cx);
        }
    }

    fn list_answered(
        &mut self,
        answer: Result<CommandAnswer, KitError>,
        cx: &mut Context<Message>,
    ) {
        let problem = match outcome(answer) {
            Ok((stdout, _)) => match logic::run_list(&stdout) {
                Some(runs) => {
                    self.runs = Some(runs);
                    self.screen = Screen::Picker;
                    self.refreshing = false;
                    self.error = None;
                    return;
                }
                None => Problem {
                    kind: ErrorKind::Error,
                    detail: stdout.chars().take(300).collect(),
                },
            },
            Err(problem) => problem,
        };
        self.screen = Screen::Error;
        self.refreshing = false;
        self.error = Some(problem);
        self.set_badge(None, cx);
    }

    fn log_answered(&mut self, answer: Result<CommandAnswer, KitError>, cx: &mut Context<Message>) {
        self.log = Some(match outcome(answer) {
            Err(problem) => LogState::Failed(problem.kind),
            Ok((stdout, _)) if stdout.trim().is_empty() => LogState::Empty,
            Ok((stdout, truncated)) => {
                let lines = Lines::new(logic::read_log(&stdout, truncated));
                // It opens on its first error by itself.
                if let Some(line) = lines.current_line() {
                    cx.scroll_to_line(self.log_id(), line as u32);
                }
                LogState::Lines(Box::new(lines))
            }
        });
    }

    /// The log of this request: another one is another widget to Hangar,
    /// which keeps where a log is scrolled by its id.
    pub fn log_id(&self) -> String {
        format!("log-{}", self.generation)
    }

    /// Moves to the previous or next error, round the ends.
    fn go_to_error(&mut self, step: isize, cx: &mut Context<Message>) {
        let id = self.log_id();
        let Some(LogState::Lines(lines)) = &mut self.log else {
            return;
        };
        let count = lines.log.errors.len() as isize;
        if count == 0 {
            return;
        }
        lines.current = (lines.current as isize + step).rem_euclid(count) as usize;
        if let Some(line) = lines.current_line() {
            cx.scroll_to_line(id, line as u32);
        }
    }
}

impl Kit for Ci {
    type Message = Message;

    fn init(start: &Start, cx: &mut Context<Message>) -> Self {
        let mut ci = Ci {
            t: strings::strings(&start.theme.locale),
            now: cx.now(),
            utc_offset: start.utc_offset,
            visible: start.visible,
            screen: Screen::Loading,
            id: None,
            from_picker: false,
            run: None,
            log: None,
            runs: None,
            refreshing: false,
            error: None,
            warning: false,
            generation: 0,
            saw_running: false,
            failures: 0,
            last_title: None,
            last_badge: None,
        };
        ci.handle_params(&start.tab.params, cx);
        ci
    }

    fn update(&mut self, message: Message, cx: &mut Context<Message>) {
        self.now = cx.now();
        match message {
            Message::Open(id) => self.open_run(id, true, cx),
            Message::Back => self.show_picker(cx),
            Message::Retry => self.retry(cx),
            // Busy, it refuses by itself rather than being disabled: a
            // disabled button would lose the keyboard.
            Message::Refresh => {
                if !self.busy() {
                    self.retry(cx)
                }
            }
            Message::Run(generation, answer) if generation == self.generation => {
                self.run_answered(answer, cx)
            }
            Message::List(generation, answer) if generation == self.generation => {
                self.list_answered(answer, cx)
            }
            Message::Log(generation, answer) if generation == self.generation => {
                self.log_answered(answer, cx)
            }
            Message::Run(..) | Message::List(..) | Message::Log(..) => {}
            Message::Poll(generation) if generation == self.generation => {
                if let Some(id) = self.id.clone() {
                    self.fetch_run(&id, cx);
                }
            }
            Message::Poll(_) | Message::Tick => {}
            Message::Visible(visible) => self.visible = visible,
            // Hangar's language changed: the badge says the run's state in the
            // new one.
            Message::Theme(theme) => {
                self.t.set_locale(theme.locale);
                if let Some(run) = &self.run {
                    let badge = logic::badge_for(run);
                    self.set_badge(Some(badge), cx);
                }
            }
            Message::Params(answer) => self.handle_params(&answer.params, cx),
            Message::PreviousError => self.go_to_error(-1, cx),
            Message::NextError => self.go_to_error(1, cx),
            Message::Lines(range) => {
                if let Some(LogState::Lines(lines)) = &mut self.log {
                    let total = lines.log.lines.len();
                    let start = (range.start as usize).min(total);
                    let end = (range.end as usize).clamp(start, (start + WINDOW_MAX).min(total));
                    lines.window = (start, end);
                }
            }
            Message::Copy(text) => cx.call(api::CopyText { text }, |copied| {
                Message::Copied(copied.is_ok())
            }),
            Message::Copied(copied) => {
                if copied {
                    cx.toast(Notice::Success, self.t.t("copied"));
                } else {
                    cx.toast(Notice::Error, self.t.t("copyFailed"));
                }
            }
        }
    }

    fn view(&self) -> Element<Message> {
        view::render(self)
    }

    fn subscriptions(&self) -> Subscriptions<Message> {
        let subscriptions = Subscriptions::none()
            .tab_params(Message::Params)
            .theme(Message::Theme)
            .visibility(Message::Visible);
        // Behind another tab nobody reads a clock; the poll goes on, since
        // the badge and the notification are for a tab nobody looks at.
        if !self.visible {
            return subscriptions;
        }
        let going = self.screen == Screen::Run && self.run.as_ref().is_some_and(|r| !r.is_done());
        if going {
            subscriptions.every("tick", TICK, Message::Tick)
        } else {
            subscriptions.every("age", AGE, Message::Tick)
        }
    }
}

export_kit!(Ci);
