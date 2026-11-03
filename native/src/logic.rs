//! The tab's pure logic: what `gh` answered, turned into what the tab shows.
//!
//! No widgets and no calls in here, so a test (or
//! a reader) sees the rules without the page around them: which state a run
//! is in, what a job is doing, why a command failed, which line of a CI log
//! says what went wrong. Saying a duration or an age is the SDK's `format`,
//! and the words are `strings.rs`.
//!
//! The log is read in one `update`, which Hangar gives 250 ms, and a failed
//! run's log is up to a megabyte of tens of thousands of lines. So the line
//! classifier is written by hand over bytes rather than as a regular
//! expression: most lines (`test x ... ok`) are turned away by one
//! comparison, and nothing is compiled or backtracked.

use hangar_kit_sdk::api::BadgeState;
use hangar_kit_sdk::format::parse_iso;
use serde_json::Value;

// ------------------------------------------------------------------ States.

/// queued, running, success, failure or neutral: for a run, a job or a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Queued,
    Running,
    Success,
    Failure,
    Neutral,
}

impl Kind {
    /// The key of the word for it in `strings.rs` (`queued`, `running`…).
    pub fn key(self) -> &'static str {
        match self {
            Kind::Queued => "queued",
            Kind::Running => "running",
            Kind::Success => "success",
            Kind::Failure => "failure",
            Kind::Neutral => "neutral",
        }
    }
}

/// The word for a state: a neutral end says what it was, when that is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Word {
    Queued,
    Running,
    Success,
    Failure,
    Neutral,
    Cancelled,
    Skipped,
}

impl Word {
    /// The word for a run (feminine in French).
    pub fn key(self) -> &'static str {
        match self {
            Word::Queued => "queued",
            Word::Running => "running",
            Word::Success => "success",
            Word::Failure => "failure",
            Word::Neutral => "neutral",
            Word::Cancelled => "cancelled",
            Word::Skipped => "skipped",
        }
    }

    /// The word for a job.
    pub fn job_key(self) -> &'static str {
        match self {
            Word::Queued => "job.queued",
            Word::Running => "job.running",
            Word::Success => "job.success",
            Word::Failure => "job.failure",
            Word::Neutral => "job.neutral",
            Word::Cancelled => "job.cancelled",
            Word::Skipped => "job.skipped",
        }
    }
}

/// What a conclusion comes to: success, failure, or neutral (cancelled,
/// skipped…).
pub fn outcome_of(conclusion: &str) -> Kind {
    match conclusion {
        "success" => Kind::Success,
        "failure" | "timed_out" | "startup_failure" => Kind::Failure,
        _ => Kind::Neutral,
    }
}

/// The state of a run, a job or a step from its `status` and `conclusion`.
pub fn kind_of(status: &str, conclusion: &str) -> Kind {
    match status {
        "completed" => outcome_of(conclusion),
        "in_progress" => Kind::Running,
        // queued, waiting, requested, pending, or unknown
        _ => Kind::Queued,
    }
}

pub fn word_of(status: &str, conclusion: &str) -> Word {
    match kind_of(status, conclusion) {
        Kind::Queued => Word::Queued,
        Kind::Running => Word::Running,
        Kind::Success => Word::Success,
        Kind::Failure => Word::Failure,
        Kind::Neutral => match conclusion {
            "cancelled" => Word::Cancelled,
            "skipped" => Word::Skipped,
            _ => Word::Neutral,
        },
    }
}

pub fn is_done(status: &str) -> bool {
    status == "completed"
}

/// What `tab.setBadge` is told: the state, and the key of the word beside it
/// (12 characters at most, in both languages).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Badge {
    pub state: BadgeState,
    pub key: &'static str,
}

pub fn badge_for(run: &Run) -> Badge {
    let (state, key) = match run.kind() {
        Kind::Running => (BadgeState::Running, "badgeRunning"),
        Kind::Queued => (BadgeState::Running, "badgeQueued"),
        Kind::Success => (BadgeState::Success, "badgeSuccess"),
        Kind::Failure => (BadgeState::Failure, "badgeFailure"),
        Kind::Neutral => (BadgeState::Neutral, "badgeNeutral"),
    };
    Badge { state, key }
}

// -------------------------------------------------------------------- Text.

/// A run id is digits and nothing else: it goes into `gh`'s argv.
pub fn is_run_id(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit())
}

/// Control, zero-width and bidi characters: what `tab.setTitle` would refuse.
fn is_invisible(c: char) -> bool {
    matches!(c,
        '\u{0000}'..='\u{001f}'
        | '\u{007f}'..='\u{009f}'
        | '\u{200b}'..='\u{200f}'
        | '\u{2028}'..='\u{202f}'
        | '\u{2060}'..='\u{206f}'
        | '\u{feff}')
}

/// One line of plain text, at most `max` characters (an ellipsis counts), as
/// `tab.setTitle` wants.
pub fn one_line(text: &str, max: usize) -> String {
    let spaced: String = text
        .chars()
        .map(|c| if is_invisible(c) { ' ' } else { c })
        .collect();
    let clean = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.chars().count() <= max {
        clean
    } else {
        let mut cut: String = clean.chars().take(max.saturating_sub(1)).collect();
        cut.push('…');
        cut
    }
}

/// `text` without the colour and cursor codes terminals take (CSI).
pub fn strip_ansi(text: &str) -> String {
    if !text.contains('\u{1b}') {
        return text.to_owned();
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let (mut copied, mut at) = (0, 0);
    while at < bytes.len() {
        if bytes[at] == 0x1b && bytes.get(at + 1) == Some(&b'[') {
            let mut end = at + 2;
            while end < bytes.len() && matches!(bytes[end], b'0'..=b'9' | b';' | b'?') {
                end += 1;
            }
            while end < bytes.len() && matches!(bytes[end], b' '..=b'/') {
                end += 1;
            }
            if end < bytes.len() && matches!(bytes[end], b'@'..=b'~') {
                out.push_str(&text[copied..at]);
                copied = end + 1;
                at = end + 1;
                continue;
            }
        }
        at += 1;
    }
    out.push_str(&text[copied..]);
    out
}

/// What `Date.parse` makes of GitHub's timestamps; GitHub writes 0001-01-01
/// for a start that has not happened, which is none.
pub fn time(value: &str) -> Option<i64> {
    parse_iso(value).filter(|ms| *ms > 86_400_000)
}

// -------------------------------------------------------------------- Runs.

/// One step of a job, as `gh run view --json jobs` gives it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Step {
    pub name: String,
    pub status: String,
    pub conclusion: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Job {
    pub name: String,
    pub status: String,
    pub conclusion: String,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub steps: Vec<Step>,
}

/// A run as `ci-run` answers it (`gh run view {id} --json …`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Run {
    pub status: String,
    pub conclusion: String,
    pub display_title: String,
    pub head_branch: String,
    pub workflow_name: String,
    pub url: String,
    pub started_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub jobs: Vec<Job>,
}

fn string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn moment(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_str).and_then(time)
}

impl Run {
    /// A run from what `gh` printed: a JSON object, read leniently (a field
    /// of another type is a field left out). None for anything else.
    pub fn parse(stdout: &str) -> Option<Run> {
        let value: Value = serde_json::from_str(stdout).ok()?;
        if !value.is_object() {
            return None;
        }
        let jobs = value
            .get("jobs")
            .and_then(Value::as_array)
            .map(|jobs| jobs.iter().filter(|job| job.is_object()).map(job).collect())
            .unwrap_or_default();
        Some(Run {
            status: string(&value, "status"),
            conclusion: string(&value, "conclusion"),
            display_title: string(&value, "displayTitle"),
            head_branch: string(&value, "headBranch"),
            workflow_name: string(&value, "workflowName"),
            url: string(&value, "url"),
            started_at: moment(&value, "startedAt"),
            updated_at: moment(&value, "updatedAt"),
            jobs,
        })
    }

    pub fn kind(&self) -> Kind {
        kind_of(&self.status, &self.conclusion)
    }

    pub fn is_done(&self) -> bool {
        is_done(&self.status)
    }
}

fn job(value: &Value) -> Job {
    let steps = value
        .get("steps")
        .and_then(Value::as_array)
        .map(|steps| {
            steps
                .iter()
                .filter(|step| step.is_object())
                .map(|step| Step {
                    name: string(step, "name"),
                    status: string(step, "status"),
                    conclusion: string(step, "conclusion"),
                })
                .collect()
        })
        .unwrap_or_default();
    Job {
        name: string(value, "name"),
        status: string(value, "status"),
        conclusion: string(value, "conclusion"),
        started_at: moment(value, "startedAt"),
        completed_at: moment(value, "completedAt"),
        steps,
    }
}

/// The progress of a job: its steps done, the step running now, the step
/// that failed.
#[derive(Debug, Clone, PartialEq)]
pub struct JobView {
    pub name: String,
    pub kind: Kind,
    pub word: Word,
    pub total: usize,
    /// The step it is at, from 1: those done, and one.
    pub at: usize,
    pub step: Option<String>,
    pub failed_step: Option<String>,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
}

pub fn job_view(job: &Job) -> JobView {
    let current = job.steps.iter().find(|step| step.status == "in_progress");
    let failed = job
        .steps
        .iter()
        .find(|step| kind_of(&step.status, &step.conclusion) == Kind::Failure);
    let done = job
        .steps
        .iter()
        .filter(|step| is_done(&step.status))
        .count();
    JobView {
        name: one_line(&job.name, 80),
        kind: kind_of(&job.status, &job.conclusion),
        word: word_of(&job.status, &job.conclusion),
        total: job.steps.len(),
        at: done + 1,
        step: current.map(|step| one_line(&step.name, 80)),
        failed_step: failed.map(|step| one_line(&step.name, 80)),
        started_at: job.started_at,
        completed_at: job.completed_at,
    }
}

/// How long a job has taken so far, or took, in ms; none before it starts.
pub fn job_duration(job: &JobView, now: i64) -> Option<i64> {
    let start = job.started_at?;
    Some((job.completed_at.unwrap_or(now) - start).max(0))
}

/// Milliseconds since the run began (until it ended, once it has), or none.
pub fn elapsed_ms(run: &Run, now: i64) -> Option<i64> {
    let start = run
        .started_at
        .into_iter()
        .chain(run.jobs.iter().filter_map(|job| job.started_at))
        .min()?;
    let mut end = now;
    if run.is_done() {
        let last = run
            .updated_at
            .into_iter()
            .chain(run.jobs.iter().filter_map(|job| job.completed_at))
            .max();
        end = last.unwrap_or(now);
    }
    Some((end - start).max(0))
}

/// Everything the page shows of one run.
#[derive(Debug, Clone, PartialEq)]
pub struct RunView {
    pub kind: Kind,
    pub word: Word,
    pub done: bool,
    pub name: String,
    pub title: String,
    pub branch: String,
    pub url: String,
    pub elapsed: Option<i64>,
    pub ended_at: Option<i64>,
    pub jobs: Vec<JobView>,
}

impl RunView {
    pub fn failed_jobs(&self) -> Vec<&JobView> {
        self.jobs
            .iter()
            .filter(|job| job.kind == Kind::Failure)
            .collect()
    }

    pub fn other_jobs(&self) -> Vec<&JobView> {
        self.jobs
            .iter()
            .filter(|job| job.kind != Kind::Failure)
            .collect()
    }
}

pub fn run_view(run: &Run, now: i64) -> RunView {
    let name = if run.workflow_name.is_empty() {
        &run.display_title
    } else {
        &run.workflow_name
    };
    RunView {
        kind: run.kind(),
        word: word_of(&run.status, &run.conclusion),
        done: run.is_done(),
        name: one_line(name, 40),
        title: one_line(&run.display_title, 200),
        branch: one_line(&run.head_branch, 80),
        url: run.url.clone(),
        elapsed: elapsed_ms(run, now),
        ended_at: if run.is_done() { run.updated_at } else { None },
        jobs: run.jobs.iter().map(job_view).collect(),
    }
}

/// One row of the picker.
#[derive(Debug, Clone, PartialEq)]
pub struct RunRow {
    pub id: String,
    pub name: String,
    pub title: String,
    pub branch: String,
    pub kind: Kind,
    pub word: Word,
    pub created_at: Option<i64>,
}

fn integer(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64().or_else(|| {
            number
                .as_f64()
                .filter(|f| f.fract() == 0.0 && f.abs() < 9.0e15)
                .map(|f| f as i64)
        }),
        _ => None,
    }
}

/// `gh run list --json …` → the picker's rows. None when it is not a list.
pub fn run_list(stdout: &str) -> Option<Vec<RunRow>> {
    let value: Value = serde_json::from_str(stdout).ok()?;
    let list = value.as_array()?;
    Some(
        list.iter()
            .filter_map(|run| {
                let id = integer(run.get("databaseId")?)?;
                let name = {
                    let workflow = string(run, "workflowName");
                    if workflow.is_empty() {
                        string(run, "displayTitle")
                    } else {
                        workflow
                    }
                };
                let status = string(run, "status");
                let conclusion = string(run, "conclusion");
                Some(RunRow {
                    id: id.to_string(),
                    name: one_line(&name, 60),
                    title: one_line(&string(run, "displayTitle"), 120),
                    branch: one_line(&string(run, "headBranch"), 60),
                    kind: kind_of(&status, &conclusion),
                    word: word_of(&status, &conclusion),
                    created_at: moment(run, "createdAt"),
                })
            })
            .collect(),
    )
}

// ---------------------------------------------------------------- Failures.

/// What went wrong, sorted into something the page can explain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    NoGh,
    NoAuth,
    NoRepo,
    RunNotFound,
    Denied,
    Unavailable,
    Timeout,
    Error,
}

impl ErrorKind {
    /// The key of its words in `strings.rs`: `err.<key>.title` and `.hint`.
    pub fn key(self) -> &'static str {
        match self {
            ErrorKind::NoGh => "no-gh",
            ErrorKind::NoAuth => "no-auth",
            ErrorKind::NoRepo => "no-repo",
            ErrorKind::RunNotFound => "run-not-found",
            ErrorKind::Denied => "denied",
            ErrorKind::Unavailable => "unavailable",
            ErrorKind::Timeout => "timeout",
            ErrorKind::Error => "error",
        }
    }

    /// What a failure gives the person to copy: where to get gh, or what to
    /// run.
    pub fn to_copy(self) -> Option<&'static str> {
        match self {
            ErrorKind::NoGh => Some("https://cli.github.com"),
            ErrorKind::NoAuth => Some("gh auth login"),
            _ => None,
        }
    }

    /// The failures that no waiting will cure.
    pub fn is_fatal(self) -> bool {
        !matches!(self, ErrorKind::Timeout | ErrorKind::Error)
    }
}

/// A failed call: Hangar's refusal, or a command's answer that is not a
/// success.
#[derive(Debug, Clone, Copy)]
pub enum Fault<'a> {
    /// The call was refused or failed: its `code`.
    Rejected { code: &'a str },
    /// The command ran: `exit_code` is none when it did not end by itself.
    Ended {
        exit_code: Option<i32>,
        stderr: &'a str,
        timed_out: bool,
    },
}

/// Whether `haystack` holds `needle`, whatever the case (ASCII).
fn mentions(haystack: &str, needle: &str) -> bool {
    find_ci(haystack.as_bytes(), needle.as_bytes(), 0).is_some()
}

/// Sorts what went wrong. Only a command's own limit times a call out, which
/// is `timed_out`.
pub fn classify(fault: Fault) -> ErrorKind {
    let (code, exit_code, stderr, timed_out) = match fault {
        Fault::Rejected { code } => (Some(code), Some(0), "", false),
        Fault::Ended {
            exit_code,
            stderr,
            timed_out,
        } => (None, exit_code, stderr, timed_out),
    };
    match code {
        Some("not_found") => return ErrorKind::NoGh,
        Some("denied") => return ErrorKind::Denied,
        Some("unavailable") => return ErrorKind::Unavailable,
        Some("busy") => return ErrorKind::Timeout,
        _ => {}
    }
    if timed_out || (code.is_none() && exit_code.is_none()) {
        return ErrorKind::Timeout;
    }
    let stderr = strip_ansi(stderr);
    let any = |needles: &[&str]| needles.iter().any(|needle| mentions(&stderr, needle));
    if any(&[
        "gh auth login",
        "not logged in",
        "GH_TOKEN",
        "authentication",
        "bad credentials",
        "HTTP 401",
    ]) {
        ErrorKind::NoAuth
    } else if any(&[
        "no git remotes",
        "not a git repository",
        "none of the git remotes",
        "unable to determine base repo",
        "could not determine base repo",
    ]) {
        ErrorKind::NoRepo
    } else if any(&[
        "could not resolve to a run",
        "could not find any workflow run",
        "HTTP 404",
        "not found",
    ]) {
        ErrorKind::RunNotFound
    } else {
        ErrorKind::Error
    }
}

// ----------------------------------------------------------------- The log.

/// The longest line kept, so one enormous line cannot stop the log from
/// being read.
pub const LINE_MAX: usize = 2000;

/// How a line of the log is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Normal,
    Error,
    Warning,
    /// A job and its step: where the lines under it come from.
    Head,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub kind: LineKind,
}

/// The log of the failed steps, readable. Nothing is cut here but a line
/// beyond reason: the page shows only what is near the view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Log {
    pub lines: Vec<Line>,
    /// The indexes of the lines that say what went wrong, in order.
    pub errors: Vec<usize>,
    /// The command's output was cut at its limit.
    pub truncated: bool,
}

fn is_digit(byte: Option<&u8>) -> bool {
    byte.is_some_and(u8::is_ascii_digit)
}

/// `2026-10-31T17:02:37.1099560Z `, and a byte-order mark before it.
fn strip_stamp(text: &str) -> &str {
    let bytes = text.as_bytes();
    let mut at = if text.starts_with('\u{feff}') { 3 } else { 0 };
    let digits = |from: usize, count: usize| (from..from + count).all(|i| is_digit(bytes.get(i)));
    if !(digits(at, 4)
        && bytes.get(at + 4) == Some(&b'-')
        && digits(at + 5, 2)
        && bytes.get(at + 7) == Some(&b'-')
        && digits(at + 8, 2)
        && bytes.get(at + 10) == Some(&b'T'))
    {
        return text;
    }
    at += 11;
    let begun = at;
    while at < bytes.len() && (bytes[at].is_ascii_digit() || bytes[at] == b':' || bytes[at] == b'.')
    {
        at += 1;
    }
    if at == begun || bytes.get(at) != Some(&b'Z') {
        return text;
    }
    at += 1;
    if bytes.get(at) == Some(&b' ') {
        at += 1;
    }
    &text[at..]
}

/// `##[error]…` and its kin: GitHub's marks, which the log shows as what they
/// mean. None for the end of a group, which says nothing.
fn read_mark(text: &str) -> Option<(&str, Option<LineKind>)> {
    let Some(rest) = text.strip_prefix("##[") else {
        return Some((text, None));
    };
    for (name, kind) in [
        ("error]", Some(LineKind::Error)),
        ("warning]", Some(LineKind::Warning)),
        ("group]", None),
    ] {
        if let Some(rest) = rest.strip_prefix(name) {
            return Some((rest, kind));
        }
    }
    if rest.starts_with("endgroup]") {
        return None;
    }
    Some((text, None))
}

/// The log of the failed steps (`gh run view --log-failed`), readable:
/// `job<TAB>step<TAB>timestamp text` lines become text, with a heading when
/// the step changes.
pub fn read_log(raw: &str, truncated: bool) -> Log {
    let stripped = strip_ansi(raw);
    let body = stripped.strip_prefix('\u{feff}').unwrap_or(&stripped);
    let mut lines: Vec<Line> = Vec::with_capacity(body.len() / 80 + 1);
    let mut errors = Vec::new();
    let mut heading: Option<String> = None;
    let mut heading_of: Option<(&str, &str)> = None;
    for line in body.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let mut parts = line.splitn(3, '\t');
        let (job, step, rest) = (parts.next(), parts.next(), parts.next());
        let columns = rest.is_some();
        let text = strip_stamp(rest.unwrap_or(line));
        if !columns && text.trim().is_empty() {
            continue;
        }
        let Some((text, mark)) = read_mark(text) else {
            continue;
        };
        if let (Some(job), Some(step), Some(_)) = (job, step, rest) {
            let pair = (job.trim(), step.trim());
            if heading_of != Some(pair) {
                heading_of = Some(pair);
                let label: String = format!("{} › {}", pair.0, pair.1)
                    .chars()
                    .take(160)
                    .collect();
                if heading.as_deref() != Some(label.as_str()) {
                    lines.push(Line {
                        text: label.clone(),
                        kind: LineKind::Head,
                    });
                    heading = Some(label);
                }
            }
        }
        let shown = if text.len() > LINE_MAX && text.chars().count() > LINE_MAX {
            let mut cut: String = text.chars().take(LINE_MAX - 1).collect();
            cut.push('…');
            cut
        } else {
            text.to_owned()
        };
        let kind = mark.unwrap_or_else(|| {
            if is_error_line(&shown) {
                LineKind::Error
            } else {
                LineKind::Normal
            }
        });
        if kind == LineKind::Error {
            errors.push(lines.len());
        }
        lines.push(Line { text: shown, kind });
    }
    Log {
        lines,
        errors,
        truncated,
    }
}

// What the tools a CI runs write when something is wrong, each at a place in
// the line where it says so: not any line that holds the word, since a list of
// « test a_refused_error ... ok » holds it hundreds of times. Each rule below
// is one alternative of the regular expression the kit's earlier HTML tab used
// (ERROR_LINE), tried without regard to case.

/// `\w`: the ASCII word characters, as JavaScript's `\w` and `\b` read them.
fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// `\s`.
fn is_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// Where `needle` first is in `haystack` from `from`, without regard to the
/// case of ASCII letters.
fn find_ci(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    let last = haystack.len().checked_sub(needle.len())?;
    let first = *needle.first()?;
    (from..=last).find(|&at| {
        haystack[at].eq_ignore_ascii_case(&first)
            && haystack[at..at + needle.len()].eq_ignore_ascii_case(needle)
    })
}

/// `\bphrase\b`: the phrase at `at`, with no word character around it.
fn phrase_at(text: &[u8], at: usize, phrase: &[u8]) -> bool {
    text.len() >= at + phrase.len()
        && text[at..at + phrase.len()].eq_ignore_ascii_case(phrase)
        && (at == 0 || !is_word(text[at - 1]))
        && text.get(at + phrase.len()).is_none_or(|b| !is_word(*b))
}

/// Where `\bphrase\b` first is in `text`, from `from`.
fn find_phrase(text: &[u8], phrase: &[u8], from: usize) -> Option<usize> {
    let mut at = from;
    while let Some(found) = find_ci(text, phrase, at) {
        if phrase_at(text, found, phrase) {
            return Some(found);
        }
        at = found + 1;
    }
    None
}

/// Whether `text` ends with `tail` (ignoring case) once its trailing
/// whitespace is gone: `tail\s*$`.
fn ends_with_ci(text: &str, tail: &str) -> bool {
    let trimmed = text.trim_end_matches(is_space).as_bytes();
    trimmed.len() >= tail.len()
        && trimmed[trimmed.len() - tail.len()..].eq_ignore_ascii_case(tail.as_bytes())
}

/// The positions where `(?:^|\s)` can be followed by something: the start,
/// and each place after a whitespace character.
fn after_space(text: &str) -> impl Iterator<Item = usize> + '_ {
    std::iter::once(0).chain(
        text.char_indices()
            .filter(|(_, c)| is_space(*c))
            .map(|(at, c)| at + c.len_utf8()),
    )
}

/// `(?:error|fatal)(?:\[[^\]]*\])?:` at `at`: `error: …`, `error[E0599]: …`,
/// `fatal: …`.
fn error_colon(text: &[u8], at: usize) -> bool {
    for word in [&b"error"[..], &b"fatal"[..]] {
        if text.len() >= at + word.len() && text[at..at + word.len()].eq_ignore_ascii_case(word) {
            let mut end = at + word.len();
            if text.get(end) == Some(&b'[')
                && let Some(close) = text[end + 1..].iter().position(|b| *b == b']')
            {
                end += close + 2;
            }
            if text.get(end) == Some(&b':') {
                return true;
            }
        }
    }
    false
}

/// `^\s*(?:[\w$]+\.)*[\w$]*(?:Error|Exception):`: `TypeError: …`,
/// `java.io.IOException: …`.
fn exception_line(text: &str) -> bool {
    let bytes = text.trim_start_matches(is_space).as_bytes();
    let mut position = 0;
    loop {
        for tail in [&b"error:"[..], &b"exception:"[..]] {
            if bytes.len() >= position + tail.len()
                && bytes[position..position + tail.len()].eq_ignore_ascii_case(tail)
            {
                return true;
            }
        }
        // The name so far is `(?:[\w$]+\.)*[\w$]*`: it grows by a word
        // character, or by a dot that follows one.
        match bytes.get(position) {
            Some(&byte) if is_word(byte) || byte == b'$' => position += 1,
            Some(b'.') if position > 0 && bytes[position - 1] != b'.' => position += 1,
            _ => return false,
        }
    }
}

/// `^\s*(?:--- )?FAIL(?:ED)?\b`: Go, Jest, Vitest, pytest.
fn fail_line(text: &str) -> bool {
    let bytes = text.trim_start_matches(is_space).as_bytes();
    let bytes = bytes.strip_prefix(b"--- ").unwrap_or(bytes);
    if !bytes
        .get(..4)
        .is_some_and(|b| b.eq_ignore_ascii_case(b"fail"))
    {
        return false;
    }
    let boundary = |at: usize| bytes.get(at).is_none_or(|b| !is_word(*b));
    (bytes
        .get(4..6)
        .is_some_and(|b| b.eq_ignore_ascii_case(b"ed"))
        && boundary(6))
        || boundary(4)
}

/// `\bassertion\b.*\bfailed\b`, on one line.
fn assertion_failed(text: &[u8]) -> bool {
    let mut from = 0;
    while let Some(at) = find_phrase(text, b"assertion", from) {
        let after = at + b"assertion".len();
        if let Some(failed) = find_phrase(text, b"failed", after) {
            let between = &text[after..failed];
            if !between.contains(&b'\n') && !between.contains(&b'\r') {
                return true;
            }
        }
        from = at + 1;
    }
    false
}

fn says_error(text: &str) -> bool {
    let bytes = text.as_bytes();
    after_space(text).any(|at| error_colon(bytes, at)) // error: …, fatal: …
        || find_phrase(bytes, b"panicked at", 0).is_some() // Rust
        || assertion_failed(bytes)
        || ends_with_ci(text, "... FAILED") // a test of Rust's
        || find_phrase(bytes, b"test result: FAILED", 0).is_some()
        || fail_line(text) // Go, Jest, Vitest, pytest
        || text
            .trim_start_matches(is_space)
            .as_bytes()
            .get(..8)
            .is_some_and(|b| b.eq_ignore_ascii_case(b"npm ERR!"))
        || exception_line(text)
}

/// What only mentions an error: a test that passed, a count of none.
fn not_an_error(text: &str) -> bool {
    let bytes = text.as_bytes();
    ends_with_ci(text, "... ok")
        || after_space(text).any(|at| {
            // `0 (?:errors?|failed|failures?)\b`
            bytes[at..].starts_with(b"0 ")
                && ["error", "errors", "failed", "failure", "failures"]
                    .iter()
                    .any(|word| phrase_at(bytes, at + 2, word.as_bytes()))
        })
        || find_phrase(bytes, b"no error", 0).is_some()
        || find_phrase(bytes, b"no errors", 0).is_some()
}

/// Whether a line of a failed step's log says what went wrong.
pub fn is_error_line(text: &str) -> bool {
    // Most lines are `test x ... ok`: turned away by one comparison.
    if ends_with_ci(text, "... ok") {
        return false;
    }
    says_error(text) && !not_an_error(text)
}
