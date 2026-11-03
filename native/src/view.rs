//! The state drawn as widgets.
//!
//! Hangar draws every widget in the person's theme, so there is no class to
//! name and no colour to choose: a state is a glyph and a word, a failure
//! is an empty state that says what to do, and the one primary action of a
//! screen is the "Try again" of a failure. Only text nodes ever carry what
//! `gh` said; nothing of it is read as markup, there being no markup.
//!
//! The view is a function of the state, `now` included, and is rebuilt after
//! every update: what must not move (a scrolled log, the keyboard's place)
//! is kept by Hangar, which keeps a widget's state while the page keeps its
//! shape.

use hangar_kit_sdk::Handler;
use hangar_kit_sdk::format;
use hangar_kit_sdk::prelude::*;
use hangar_kit_sdk::ui::{LineKind as Shown, ListItem};

use crate::logic::{self, ErrorKind, JobView, Kind, LineKind, RunRow};
use crate::{Ci, Lines, LogState, Message, Problem, Screen};

/// How tall the log of a failed run grows before it scrolls, in pixels. A
/// native page has no `vh`.
pub const LOG_HEIGHT: f32 = 360.0;

/// The glyph of a state, which says it by shape as well as colour.
pub fn status(kind: Kind) -> Status {
    match kind {
        Kind::Queued => Status::Waiting,
        Kind::Running => Status::Running,
        Kind::Success => Status::Success,
        Kind::Failure => Status::Failure,
        Kind::Neutral => Status::Neutral,
    }
}

pub fn render(ci: &Ci) -> Element<Message> {
    let body: Element<Message> = match (ci.screen, &ci.error) {
        (Screen::Error, Some(problem)) => problem_view(ci, problem),
        (Screen::Picker, _) => picker(ci),
        (Screen::Run, _) if ci.run.is_some() => run_body(ci),
        _ => loading(ci.t.t(if ci.id.is_some() {
            "loadingRun"
        } else {
            "loadingRuns"
        }))
        .into(),
    };
    let mut parts: Vec<Element<Message>> = vec![head(ci), scroll(body).into()];
    if let Some(foot) = foot(ci) {
        parts.push(foot);
    }
    page(parts).fill().into()
}

/// "name on branch", or whichever of the two there is.
fn place(ci: &Ci, name: &str, branch: &str) -> String {
    if !name.is_empty() && !branch.is_empty() {
        ci.t.tv("onBranch", &[("name", name), ("branch", branch)])
    } else if !name.is_empty() {
        name.to_owned()
    } else {
        branch.to_owned()
    }
}

// What every state shares: a head that does not jump, a body, a foot.
fn head(ci: &Ci) -> Element<Message> {
    let t = &ci.t;
    let shown = match (&ci.run, ci.screen) {
        (Some(run), Screen::Run | Screen::Error) => Some(logic::run_view(run, ci.now)),
        _ => None,
    };
    let title = match (&ci.id, &shown) {
        (Some(_), Some(view)) if !view.title.is_empty() || !view.name.is_empty() => {
            if view.title.is_empty() {
                view.name.clone()
            } else {
                view.title.clone()
            }
        }
        (Some(id), _) => t.tv("run", &[("id", id)]),
        (None, _) => t.t("runs"),
    };
    let mut head = header(title);
    if ci.id.is_some() && ci.from_picker {
        head = head.lead(icon_button(Icon::ArrowLeft, t.t("back")).on_press(Message::Back));
    }
    if ci.id.is_none() {
        // A first read says so in the page already: one ring is enough.
        if ci.refreshing {
            head = head.action(glyph(Status::Running, t.t("refreshing")));
        }
        head = head.action(icon_button(Icon::Restart, t.t("refresh")).on_press(Message::Refresh));
    }
    match (&shown, ci.screen) {
        (Some(view), Screen::Run) => {
            head = head.state(state_label(status(view.kind), t.t(view.word.key())));
            let mut facts = Vec::new();
            if let Some(elapsed) = view.elapsed {
                facts.push(format::duration(elapsed as f64, t.locale()));
            }
            if let Some(ended) = view.ended_at {
                facts.push(format::since(ended, ci.now, ci.utc_offset, t.locale()));
            }
            let place = place(ci, &view.name, &view.branch);
            if !place.is_empty() {
                facts.push(place);
            }
            if !facts.is_empty() {
                head = head.state(text(facts.join(", ")).small().muted());
            }
        }
        // Loading a run, its second line is kept, so the head does not grow
        // once it is read.
        (_, Screen::Loading) if ci.id.is_some() => {
            head = head.state(text("\u{a0}").small());
        }
        _ => {}
    }
    head.into()
}

fn problem_view(ci: &Ci, problem: &Problem) -> Element<Message> {
    let t = &ci.t;
    let key = problem.kind.key();
    let title = t.t(&format!("err.{key}.title"));
    let mut said = if problem.kind == ErrorKind::Denied {
        denied(title)
    } else {
        failed(title)
    };
    said = said.text(t.t(&format!("err.{key}.hint")));
    // Something to copy, on a line of its own: where to get gh, or what to run.
    if let Some(to_copy) = problem.kind.to_copy() {
        said = said.child(
            row![
                text(to_copy).code(),
                icon_button(Icon::Copy, t.t("copy")).on_press(Message::Copy(to_copy.to_owned())),
            ]
            .spacing(4.0)
            .align(Align::Center),
        );
    }
    if problem.kind == ErrorKind::Error && !problem.detail.is_empty() {
        said = said.child(pre(problem.detail.clone()));
    }
    said.action(button(t.t("retry")).primary().on_press(Message::Retry))
        .into()
}

fn picker(ci: &Ci) -> Element<Message> {
    let t = &ci.t;
    let runs: &[RunRow] = ci.runs.as_deref().unwrap_or_default();
    if runs.is_empty() {
        return empty(t.t("noRuns.title")).text(t.t("noRuns.hint")).into();
    }
    list(runs.iter().map(|run| {
        // The age goes under the title rather than beside it: « yesterday at
        // 9:13 PM » would leave a title little room at the panel's narrowest,
        // and the title says which run.
        let age = run
            .created_at
            .map(|at| format::since(at, ci.now, ci.utc_offset, t.locale()))
            .unwrap_or_default();
        let title = if run.title.is_empty() {
            &run.name
        } else {
            &run.title
        };
        item(title.clone())
            .lead(glyph(status(run.kind), t.t(run.word.key())))
            .body(row![
                text(place(ci, &run.name, &run.branch))
                    .small()
                    .dim()
                    .truncate(),
                fill(),
                text(age).small().dim().numeric(),
            ])
            .on_press(Message::Open(run.id.clone()))
    }))
    .into()
}

fn run_body(ci: &Ci) -> Element<Message> {
    let t = &ci.t;
    let Some(run) = &ci.run else {
        return loading(t.t("loadingRun")).into();
    };
    let view = logic::run_view(run, ci.now);
    let mut parts: Vec<Element<Message>> = Vec::new();
    if ci.warning {
        parts.push(banner(Notice::Warning, t.t("warning")).into());
    }
    // On a failure, what failed comes first, with its log right under it.
    let failed = view.kind == Kind::Failure && !view.failed_jobs().is_empty();
    let (first, others) = if failed {
        (view.failed_jobs(), view.other_jobs())
    } else {
        (view.jobs.iter().collect(), Vec::new())
    };
    let jobs = if first.is_empty() {
        vec![
            text(t.t(if view.done { "noJobsRan" } else { "noJobs" }))
                .small()
                .dim()
                .into(),
        ]
    } else {
        vec![job_rows(ci, &first)]
    };
    let mut section_of_jobs = section(t.t(if failed { "failedJobs" } else { "jobs" }), jobs);
    if !first.is_empty() {
        section_of_jobs = section_of_jobs.count(first.len().to_string());
    }
    parts.push(section_of_jobs.into());
    if view.kind == Kind::Failure {
        parts.push(log_section(ci));
    }
    if !others.is_empty() {
        parts.push(
            section(t.t("otherJobs"), [job_rows(ci, &others)])
                .count(others.len().to_string())
                .into(),
        );
    }
    column(parts).spacing(12.0).into()
}

fn job_rows(ci: &Ci, jobs: &[&JobView]) -> Element<Message> {
    list(jobs.iter().map(|job| job_row(ci, job))).into()
}

/// A job: its state's glyph, its name, the step it is at, how long it took.
fn job_row(ci: &Ci, job: &JobView) -> ListItem<Handler<Message>> {
    let t = &ci.t;
    let said = t.t(job.word.job_key());
    // It wraps: the step a job is at is what its row is read for, at any
    // width.
    let mut row = item(job.name.clone())
        .lead(glyph(status(job.kind), said.clone()))
        .wrap();
    let sub = match (&job.step, &job.failed_step) {
        (Some(step), _) => Some(t.tv(
            "stepRunning",
            &[
                ("step", step),
                ("at", &job.at.to_string()),
                ("total", &job.total.to_string()),
            ],
        )),
        (None, Some(step)) if job.kind == Kind::Failure => {
            Some(t.tv("stepFailed", &[("step", step)]))
        }
        _ => None,
    };
    if let Some(sub) = sub {
        row = row.sub(sub);
    }
    // A job that did not run has no time to give: its state's word instead.
    let end = match logic::job_duration(job, ci.now) {
        Some(ms) if job.kind != Kind::Neutral => format::duration(ms as f64, t.locale()),
        _ => said,
    };
    row.end(text(end).numeric().dim())
}

fn log_section(ci: &Ci) -> Element<Message> {
    let t = &ci.t;
    let body: Vec<Element<Message>> = match &ci.log {
        None => vec![loading(t.t("logLoading")).into()],
        Some(LogState::Empty) => vec![
            empty(t.t("logEmpty.title"))
                .text(t.t("logEmpty.hint"))
                .into(),
        ],
        Some(LogState::Failed(kind)) => {
            let reason = t.t(&format!("err.{}.title", kind.key()));
            vec![
                banner(Notice::Error, t.tv("logError", &[("reason", &reason)]))
                    .action(button(t.t("retry")).small().on_press(Message::Retry))
                    .into(),
            ]
        }
        Some(LogState::Lines(lines)) => lines_view(ci, lines),
    };
    section(t.t("failedLog"), body).into()
}

fn lines_view(ci: &Ci, lines: &Lines) -> Vec<Element<Message>> {
    let t = &ci.t;
    let mut parts: Vec<Element<Message>> = Vec::new();
    if lines.log.truncated {
        parts.push(banner(Notice::Warning, t.t("logCut")).into());
    }
    // How many errors, and a way between them: the log opens on the first.
    let errors = lines.log.errors.len();
    if errors > 0 {
        let said = if errors > 1 {
            t.tv(
                "errorAt",
                &[
                    ("at", &(lines.current + 1).to_string()),
                    ("n", &errors.to_string()),
                ],
            )
        } else {
            t.tn("errors", errors as i64, &[])
        };
        let mut bar = row![text(said).small().dim(), fill()]
            .spacing(4.0)
            .align(Align::Center);
        if errors > 1 {
            bar = bar
                .push(
                    button(t.t("previous"))
                        .small()
                        .ghost()
                        .on_press(Message::PreviousError),
                )
                .push(
                    button(t.t("next"))
                        .small()
                        .ghost()
                        .on_press(Message::NextError),
                );
        }
        parts.push(bar.into());
    }
    let (start, end) = lines.window;
    let current = lines.current_line();
    let shown = lines.log.lines[start..end]
        .iter()
        .enumerate()
        .map(|(offset, line)| {
            log_line(line.text.clone())
                .kind(match line.kind {
                    LineKind::Normal => Shown::Normal,
                    LineKind::Error => Shown::Error,
                    LineKind::Warning => Shown::Warning,
                    LineKind::Head => Shown::Head,
                })
                .current(current == Some(start + offset))
        });
    let mut log = log_lines(shown)
        .id(ci.log_id())
        .height(LOG_HEIGHT)
        .window(start as u32, lines.log.lines.len() as u32)
        .on_range(Message::Lines);
    if errors == 0 {
        log = log.at_end();
    }
    parts.push(log.into());
    parts
}

/// The link, a quiet line at the foot, which can be copied: a native tab
/// opens no link.
fn foot(ci: &Ci) -> Option<Element<Message>> {
    if ci.screen != Screen::Run {
        return None;
    }
    let url = &ci.run.as_ref()?.url;
    if url.is_empty() {
        return None;
    }
    // A bar wraps what does not fit, so the line is a row of its own in it:
    // the link gives way, the button stays.
    Some(
        bar([row![
            container(tooltip(
                text(url.clone()).small().dim().truncate(),
                url.clone(),
            ))
            .width(Length::Fill),
            icon_button(Icon::Copy, ci.t.t("copyLink")).on_press(Message::Copy(url.clone())),
        ]
        .spacing(6.0)
        .align(Align::Center)
        .into()])
        .foot()
        .into(),
    )
}
