//! What the page draws for each state, as the widgets Hangar draws: a glyph
//! and a word for a state, every state designed, both languages, nothing of
//! what gh said taken for anything but text. `test/view.test.js`, ported,
//! case for case; where the page's test looked at classes and DOM nodes,
//! this one looks at the widgets.

mod support;

use hangar_kit_sdk::ui::{Element, EmptyKind, LineKind as Shown, Notice, ProgressKind, Status};
use kit_ci_tab::{LogState, POLL};
use serde_json::json;
use support::*;

/// Where the page's view tests stood: 18:48:37 UTC, 31 October 2026.
const NOW: &str = "2026-10-31T18:48:37Z";

fn failed_run() -> Setup {
    Setup::new()
        .now(NOW)
        .serves("ci-run", RUN_FAILURE)
        .serves("ci-log", LOG_FAILED)
}

fn state_of(fake: &Fake) -> (Status, String) {
    fake.header()
        .state
        .iter()
        .find_map(|part| match part {
            Element::State(state) => Some((state.status, state.label.clone())),
            _ => None,
        })
        .expect("a state in the head")
}

#[test]
fn the_runs_title_and_state_stand_in_the_head_what_it_took_under_them() {
    let fake = failed_run().start();
    let head = fake.header();
    assert_eq!(head.title, "Release 1.7.0");
    assert_eq!(state_of(&fake), (Status::Failure, "Failed".to_owned()));
    let line = head
        .state
        .iter()
        .map(said)
        .find(|line| !line.is_empty() && line != "Failed");
    assert_eq!(
        line.as_deref(),
        Some("4\u{a0}min\u{a0}16\u{a0}s, today at 5:04\u{202f}PM, CI on main")
    );
    assert!(head.lead.is_empty(), "opened by a hook: no way back");
    assert!(
        head.actions.is_empty(),
        "and no refresh: the run polls by itself"
    );
}

#[test]
fn a_running_run_jobs_as_rows_with_their_glyph_and_time_the_running_step_in_its_row() {
    let fake = Setup::new().now(NOW).serves("ci-run", RUN_RUNNING).start();
    assert_eq!(state_of(&fake).0, Status::Running);
    let rows = fake.items();
    let titles: Vec<_> = rows.iter().map(|row| row.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "rust (windows-latest)",
            "rust (macos-latest)",
            "frontend",
            "rust (ubuntu-22.04)"
        ]
    );
    let glyphs = fake.glyphs();
    assert_eq!(
        glyphs.iter().map(|g| g.status).collect::<Vec<_>>(),
        [
            Status::Success,
            Status::Running,
            Status::Waiting,
            Status::Waiting
        ]
    );
    assert_eq!(
        glyphs.iter().map(|g| g.label.as_str()).collect::<Vec<_>>(),
        ["Passed", "Running", "Queued", "Queued"],
        "the glyph is named, for whoever cannot see its shape"
    );
    assert!(
        rows.iter().all(|row| row.wrap),
        "the step a job is at is read at any width"
    );

    assert_eq!(
        rows[1].sub.as_deref(),
        Some("Keep Defender out of the build, step 3 of 13")
    );
    assert_eq!(
        rows[1].end.as_deref().map(said).as_deref(),
        Some("1\u{a0}min\u{a0}41\u{a0}s"),
        "how long it has been going"
    );
    assert_eq!(rows[2].sub, None, "a queued job has no step");
    assert_eq!(
        rows[2].end.as_deref().map(said).as_deref(),
        Some("Queued"),
        "and no time"
    );
    assert!(
        !fake
            .widgets()
            .iter()
            .any(|w| matches!(w, Element::Progress(_))),
        "no bar: a state is a glyph and a word"
    );
    assert!(fake.log().is_none(), "no log while it runs");
    assert!(fake.ticks("tick"), "the times run on");
}

#[test]
fn a_cancelled_run_and_a_skipped_job_say_so_rather_than_ended() {
    let mut run: serde_json::Value = serde_json::from_str(RUN_SUCCESS).expect("json");
    run["conclusion"] = json!("cancelled");
    run["jobs"][0]["conclusion"] = json!("skipped");
    let fake = Setup::new()
        .command("ci-run", move |_| ok(run.to_string()))
        .start();
    assert_eq!(state_of(&fake), (Status::Neutral, "Cancelled".to_owned()));
    assert_eq!(
        fake.items()[0].end.as_deref().map(said).as_deref(),
        Some("Skipped")
    );
}

#[test]
fn a_failure_what_failed_first_its_log_right_under_it_then_the_rest() {
    let fake = failed_run().start();
    assert_eq!(
        fake.sections(),
        [
            ("Failed jobs".to_owned(), Some("1".to_owned())),
            ("Log of the failed steps".to_owned(), None),
            ("Other jobs".to_owned(), Some("3".to_owned())),
        ]
    );
    let rows = fake.items();
    assert_eq!(rows[0].title, "rust (macos-latest)");
    assert_eq!(rows[0].sub.as_deref(), Some("Failed at the step Tests"));
    assert_eq!(
        rows[0].end.as_deref().map(said).as_deref(),
        Some("2\u{a0}min\u{a0}23\u{a0}s")
    );
    assert_eq!(rows.len(), 4);
    assert_eq!(fake.glyphs()[0].status, Status::Failure);
}

#[test]
fn the_log_is_told_which_lines_are_errors_and_opens_on_the_first() {
    let fake = failed_run().start();
    let log = fake.log().expect("the log");
    assert_eq!(log.id, fake.tab.kit().log_id());
    assert_eq!(log.total, Some(151));
    assert!(log.on_range.is_some(), "given in parts");
    assert!(!log.at_end);
    let errors: Vec<_> = log
        .lines
        .iter()
        .filter(|line| line.kind == Shown::Error)
        .collect();
    assert_eq!(errors.len(), 5);
    assert!(errors[0].text.contains("panicked at src/accounts.rs:688:9"));
    let current: Vec<_> = log.lines.iter().filter(|line| line.current).collect();
    assert_eq!(current.len(), 1, "one error is the one looked at");
    assert_eq!(current[0].text, errors[0].text);
    assert!(
        log.lines.iter().all(|line| line.number.is_none()),
        "no numbers"
    );
    assert!(log.lines.iter().any(|line| line.kind == Shown::Normal));
}

#[test]
fn the_log_every_way_it_can_go() {
    // Being read.
    let mut waiting = Setup::new()
        .serves("ci-run", RUN_FAILURE)
        .command("ci-log", |_| Said::Held)
        .start();
    assert!(waiting.tab.shows("Reading the log…"));
    assert!(waiting.log().is_none());
    assert!(
        waiting
            .empties()
            .iter()
            .any(|e| e.kind == EmptyKind::Loading)
    );
    waiting.release("ci-log", ok(LOG_FAILED));
    assert!(waiting.log().is_some());
    assert!(!waiting.tab.shows("Reading the log…"));

    // Unreadable: the result stands, and the log says why.
    let unreadable = Setup::new()
        .serves("ci-run", RUN_FAILURE)
        .command("ci-log", |_| fails(1, "run 1 is still in progress"))
        .start();
    let banners = unreadable.banners();
    assert_eq!(banners.len(), 1);
    assert_eq!(banners[0].kind, Notice::Error);
    assert_eq!(banners[0].text, "The log could not be read: gh failed.");
    assert!(
        unreadable
            .buttons()
            .iter()
            .any(|b| b.label.as_deref() == Some("Try again"))
    );

    // Blank.
    let blank = Setup::new()
        .serves("ci-run", RUN_FAILURE)
        .serves("ci-log", "  \n")
        .start();
    assert!(blank.tab.shows("The log is empty"));
    assert!(blank.tab.shows("GitHub may still be storing it."));
    assert!(blank.log().is_none());

    // Cut at the command's limit.
    let cut_log = Setup::new()
        .serves("ci-run", RUN_FAILURE)
        .command("ci-log", |_| cut("a line\n"))
        .start();
    let banners = cut_log.banners();
    assert_eq!(banners.len(), 1);
    assert_eq!(banners[0].kind, Notice::Warning);
    assert!(banners[0].text.contains("first megabyte"));
}

#[test]
fn the_link_is_a_quiet_line_at_the_foot_and_can_be_copied() {
    let mut fake = failed_run().start();
    let foot = fake.foot().expect("a foot");
    let url = "https://github.com/hangar-ai/hangar/actions/runs/41222364482";
    let texts: Vec<_> = {
        let mut found = Vec::new();
        for child in &foot.children {
            child.for_each(&mut |e| {
                if let Element::Text(text) = e {
                    found.push(text.clone());
                }
            });
        }
        found
    };
    assert_eq!(texts.len(), 1);
    assert_eq!(texts[0].text, url);
    assert_eq!(
        texts[0].wrap,
        hangar_kit_sdk::ui::Wrap::Truncate,
        "it gives way, the button stays"
    );
    let mut copy = None;
    for child in &foot.children {
        child.for_each(&mut |e| {
            if let Element::Button(button) = e {
                copy = Some(button.clone());
            }
        });
    }
    let copy = copy.expect("a copy button");
    assert_eq!(
        copy.tooltip.as_deref(),
        Some("Copy the link"),
        "an icon alone is named"
    );
    fake.press("Copy the link");
    assert_eq!(fake.calls_to("ui.copy")[0].params["text"], url);
}

#[test]
fn a_run_that_fell_over_hides_what_it_said_of_itself_and_the_link() {
    let mut replies = vec![ok(RUN_RUNNING), fails(4, NOT_LOGGED_IN)].into_iter();
    let mut fake = Setup::new()
        .command("ci-run", move |_| replies.next().expect("two calls"))
        .start();
    assert!(fake.tab.shows("Running"));
    fake.advance(POLL.as_millis() as i64);
    assert_eq!(fake.tab.kit().screen, kit_ci_tab::Screen::Error);
    assert!(!fake.tab.shows("Running"));
    assert!(fake.foot().is_none());
    assert!(fake.tab.shows("gh is not signed in"));
    assert!(fake.log().is_none());
}

#[test]
fn a_hiccup_while_following_a_run_is_a_banner_over_the_run() {
    let mut replies = vec![ok(RUN_RUNNING), fails(1, "error connecting")].into_iter();
    let mut fake = Setup::new()
        .command("ci-run", move |_| {
            replies.next().unwrap_or_else(|| ok(RUN_RUNNING))
        })
        .start();
    assert!(fake.banners().is_empty());
    fake.advance(POLL.as_millis() as i64);
    let banners = fake.banners();
    assert_eq!(banners.len(), 1);
    assert_eq!(banners[0].kind, Notice::Warning);
    assert_eq!(banners[0].text, "The last refresh failed. Trying again…");
    assert!(
        fake.tab.shows("rust (macos-latest)"),
        "the run stays under it"
    );
    fake.advance(POLL.as_millis() as i64);
    assert!(fake.banners().is_empty());
}

#[test]
fn in_french_with_accents() {
    let fake = failed_run().lang("fr").start();
    assert_eq!(state_of(&fake), (Status::Failure, "Échouée".to_owned()));
    assert!(fake.tab.shows("Journal des étapes en échec"));
    assert!(fake.tab.shows("Jobs en échec"));
    assert!(fake.tab.shows("Échec à l'étape Tests"));
    assert!(fake.tab.shows("CI sur main"));
    assert_eq!(fake.glyphs()[0].label, "Échoué", "a job agrees as a job");
    assert!(fake.tab.shows("Erreur 1 sur 5"));
    assert!(fake.tab.shows("Suivante"));
    assert!(
        fake.tab
            .shows("4\u{a0}min\u{a0}16\u{a0}s, aujourd'hui à 17\u{a0}h\u{a0}04, CI sur main")
    );
}

#[test]
fn a_run_with_no_job_has_no_count_of_them() {
    let mut queued: serde_json::Value = serde_json::from_str(RUN_RUNNING).expect("json");
    queued["status"] = json!("queued");
    queued["jobs"] = json!([]);
    let fake = Setup::new()
        .command("ci-run", move |_| ok(queued.to_string()))
        .start();
    assert!(fake.tab.shows("No job yet."));
    assert_eq!(fake.sections(), [("Jobs".to_owned(), None)]);
}

#[test]
fn loading_says_what_it_reads_under_a_head_that_will_not_move() {
    let run = Setup::new()
        .params(&[("id", "1")])
        .command("ci-run", |_| Said::Held)
        .start();
    assert!(run.tab.shows("Reading the run…"));
    assert_eq!(run.header().title, "Run #1");
    assert_eq!(run.empties().len(), 1);
    assert_eq!(run.empties()[0].kind, EmptyKind::Loading);

    let list = Setup::new()
        .params(&[])
        .command("ci-list", |_| Said::Held)
        .start();
    assert!(list.tab.shows("Reading the runs…"));
    assert_eq!(list.header().title, "Recent runs");
    let refresh = list.header().actions;
    assert_eq!(
        refresh.len(),
        1,
        "one ring is enough: a first read says so in the page already"
    );
    assert!(matches!(&refresh[0], Element::Button(b) if b.tooltip.as_deref() == Some("Refresh")));
}

#[test]
fn the_head_of_a_run_loading_already_holds_the_height_of_its_second_line() {
    let fake = Setup::new().command("ci-run", |_| Said::Held).start();
    let head = fake.header();
    assert_eq!(head.state.len(), 1);
    assert_eq!(said(&head.state[0]), "\u{a0}");
}

#[test]
fn refreshing_keeps_the_rows_and_says_it_is_working() {
    let mut fake = Setup::new()
        .params(&[])
        .command("ci-list", |_| Said::Held)
        .start();
    fake.release("ci-list", ok(RUN_LIST));
    let rows = fake.items();
    assert_eq!(rows.len(), 15);
    assert!(
        fake.header()
            .actions
            .iter()
            .all(|a| !matches!(a, Element::Glyph(_)))
    );

    fake.press("Refresh");
    assert_eq!(fake.runs_of("ci-list").len(), 2);
    let working = fake
        .header()
        .actions
        .into_iter()
        .find_map(|a| match a {
            Element::Glyph(glyph) => Some(glyph),
            _ => None,
        })
        .expect("a ring");
    assert_eq!(working.status, Status::Running);
    assert_eq!(working.label, "Refreshing…");
    assert_eq!(fake.items(), rows, "the rows stay");
    assert!(
        fake.buttons()
            .iter()
            .any(|b| b.tooltip.as_deref() == Some("Refresh") && b.on_press.is_some()),
        "not disabled: that would take the keyboard away"
    );

    fake.press("Refresh");
    assert_eq!(fake.runs_of("ci-list").len(), 2, "once is enough");

    fake.release("ci-list", ok(RUN_LIST));
    assert!(
        fake.header()
            .actions
            .iter()
            .all(|a| !matches!(a, Element::Glyph(_)))
    );
    assert_eq!(fake.items(), rows, "the same runs");
}

/// Name, what it does, its title, the icon of its kind, what to copy.
type Case = (
    &'static str,
    fn() -> Setup,
    &'static str,
    EmptyKind,
    Option<&'static str>,
);

fn cases() -> Vec<Case> {
    vec![
        (
            "not logged in",
            || Setup::new().command("ci-run", |_| fails(4, NOT_LOGGED_IN)),
            "gh is not signed in",
            EmptyKind::Error,
            Some("gh auth login"),
        ),
        (
            "gh absent",
            || Setup::new().command("ci-run", |_| Said::Refused("not_found")),
            "gh is not installed",
            EmptyKind::Error,
            Some("https://cli.github.com"),
        ),
        (
            "permission denied",
            Setup::new,
            "This tab may not read the CI",
            EmptyKind::Denied,
            None,
        ),
        (
            "no repository",
            || Setup::new().command("ci-run", |_| fails(1, "fatal: not a git repository")),
            "No GitHub repository here",
            EmptyKind::Error,
            None,
        ),
        (
            "no such run",
            || Setup::new().command("ci-run", |_| fails(1, "HTTP 404: not found")),
            "Run not found",
            EmptyKind::Error,
            None,
        ),
        (
            "too long",
            || Setup::new().command("ci-run", |_| timed_out()),
            "gh took too long",
            EmptyKind::Error,
            None,
        ),
    ]
}

#[test]
fn every_failure_is_a_designed_state_what_it_is_what_to_do_and_one_action() {
    for (name, setup, title, kind, to_copy) in cases() {
        let fake = setup().start();
        let empties = fake.empties();
        assert_eq!(empties.len(), 1, "{name}");
        let said_here = &empties[0];
        assert_eq!(said_here.kind, kind, "{name}");
        assert_eq!(said_here.title, title, "{name}");
        assert!(
            said_here.text.as_deref().is_some_and(|t| t.len() > 20),
            "{name}"
        );
        let primary: Vec<_> = fake
            .buttons()
            .into_iter()
            .filter(|b| b.kind == hangar_kit_sdk::ui::ButtonKind::Primary)
            .collect();
        assert_eq!(primary.len(), 1, "{name}: one primary action");
        assert_eq!(primary[0].label.as_deref(), Some("Try again"), "{name}");
        match to_copy {
            Some(text) => {
                let codes: Vec<_> = said_here
                    .children
                    .iter()
                    .flat_map(|child| {
                        let mut found = Vec::new();
                        child.for_each(&mut |e| {
                            if let Element::Text(t) = e
                                && t.code
                            {
                                found.push(t.text.clone());
                            }
                        });
                        found
                    })
                    .collect();
                assert_eq!(codes, [text], "{name}");
            }
            None => assert!(said_here.children.is_empty(), "{name}"),
        }
        assert!(fake.foot().is_none(), "{name}");
        assert!(fake.log().is_none(), "{name}");

        // In French as well.
        let french = setup().lang("fr").start();
        assert_ne!(french.empties()[0].title, title, "{name}");
        assert!(
            french
                .buttons()
                .iter()
                .any(|b| b.label.as_deref() == Some("Réessayer")),
            "{name}"
        );
    }
}

#[test]
fn the_copy_line_of_a_failure_copies_it() {
    let mut fake = Setup::new()
        .command("ci-run", |_| fails(4, NOT_LOGGED_IN))
        .start();
    assert!(
        fake.buttons()
            .iter()
            .any(|b| b.tooltip.as_deref() == Some("Copy"))
    );
    fake.press("Copy");
    assert_eq!(fake.calls_to("ui.copy")[0].params["text"], "gh auth login");
}

#[test]
fn what_gh_said_when_nothing_else_explains_it_is_shown_as_text() {
    let fake = Setup::new()
        .command("ci-run", |_| fails(1, "something <b>odd</b>"))
        .start();
    let empties = fake.empties();
    assert_eq!(empties[0].title, "gh failed");
    assert_eq!(empties[0].text.as_deref(), Some("This is what it said:"));
    // « This is what it said: » comes right before what it said, the action after.
    assert_eq!(empties[0].children.len(), 1);
    assert!(
        matches!(&empties[0].children[0], Element::Pre(pre) if pre.text == "something <b>odd</b>")
    );
    assert_eq!(empties[0].actions.len(), 1);
}

#[test]
fn the_picker_lists_runs_as_rows_with_branch_and_age_that_open_them() {
    let mut fake = Setup::new()
        .now(NOW)
        .params(&[])
        .serves("ci-list", RUN_LIST)
        .serves("ci-run", RUN_SUCCESS)
        .start();
    assert_eq!(fake.header().title, "Recent runs");
    let rows = fake.items();
    assert_eq!(rows.len(), 15);
    assert_eq!(rows[0].title, "Point the site's downloads to 1.8.0");
    let line: Vec<_> = rows[0]
        .body
        .iter()
        .flat_map(|part| match part {
            Element::Row(row) => row.children.iter().map(said).collect::<Vec<_>>(),
            other => vec![said(other)],
        })
        .filter(|t| !t.is_empty())
        .collect();
    assert_eq!(line, ["CI on main", "1\u{a0}min ago"]);
    let second: Vec<_> = rows[1].body.iter().map(said).collect();
    assert!(second[0].starts_with("Release on v1.8.0"), "{second:?}");
    assert_eq!(fake.glyphs()[0].label, "Passed");
    assert!(fake.foot().is_none());
    assert!(fake.log().is_none());

    fake.press("Point the site's downloads to 1.8.0");
    assert_eq!(
        fake.runs_of("ci-run")[0].params["args"],
        json!({ "id": "41231065926" })
    );
}

#[test]
fn no_run_says_so_and_what_will_bring_one() {
    let fake = Setup::new().params(&[]).serves("ci-list", "[]").start();
    let empties = fake.empties();
    assert_eq!(empties.len(), 1);
    assert_eq!(empties[0].kind, EmptyKind::Empty, "not an error");
    assert_eq!(empties[0].title, "No run yet");
    assert!(
        empties[0]
            .text
            .as_deref()
            .is_some_and(|t| t.starts_with("A push to this repository"))
    );
    assert!(
        fake.buttons()
            .iter()
            .any(|b| b.tooltip.as_deref() == Some("Refresh") && b.on_press.is_some())
    );
}

#[test]
fn a_run_opened_from_the_picker_has_a_way_back_one_opened_by_a_hook_has_none() {
    let mut fake = Setup::new()
        .params(&[])
        .serves("ci-list", RUN_LIST)
        .serves("ci-run", RUN_SUCCESS)
        .start();
    assert!(fake.header().lead.is_empty());
    fake.press("Point the site's downloads to 1.8.0");
    let lead = fake.header().lead;
    assert_eq!(lead.len(), 1);
    assert!(
        matches!(&lead[0], Element::Button(b) if b.tooltip.as_deref() == Some("Back to the recent runs") && b.on_press.is_some())
    );
    assert!(fake.header().actions.is_empty());
    let direct = Setup::new().serves("ci-run", RUN_SUCCESS).start();
    assert!(direct.header().lead.is_empty());
}

#[test]
fn a_redraw_leaves_alone_what_did_not_change_only_the_clocks_move() {
    let mut fake = Setup::new().now(NOW).serves("ci-run", RUN_RUNNING).start();
    let before_rows = fake.items();
    let before_head = fake.header();
    let before_foot = fake.foot();
    fake.advance(1000);
    let rows = fake.items();
    assert_eq!(rows[0], before_rows[0], "a job over: the same row");
    assert_eq!(rows[2], before_rows[2], "a job waiting: the same row");
    assert_ne!(
        rows[1].end, before_rows[1].end,
        "the job going: its time moved"
    );
    assert_ne!(
        fake.header().state,
        before_head.state,
        "the run's time moved"
    );
    assert_eq!(fake.header().title, before_head.title);
    assert_eq!(fake.foot(), before_foot, "the same button, focus and all");
}

#[test]
fn a_log_is_given_once_a_redraw_never_scrolls_it_back() {
    let mut fake = failed_run().start();
    let log = fake.log().expect("a log");
    assert!(
        !fake.tab.take_operations().is_empty(),
        "it opened on its first error"
    );
    fake.event("tab.visibility", json!({ "visible": false }));
    fake.event("tab.visibility", json!({ "visible": true }));
    fake.advance(60_000);
    assert_eq!(
        fake.log().expect("a log"),
        log,
        "the same widget, the same lines"
    );
    assert!(
        fake.tab.take_operations().is_empty(),
        "and nothing scrolls it"
    );
}

#[test]
fn hangars_language_changing_redraws_the_page_in_it() {
    let mut fake = failed_run().start();
    assert!(fake.tab.shows("Failed jobs"));
    let log = fake.log().expect("a log");
    fake.speak("fr");
    assert!(fake.tab.shows("Jobs en échec"));
    assert!(!fake.tab.shows("Failed jobs"));
    assert_eq!(
        fake.log().expect("still"),
        log,
        "the log is the same widget"
    );
    assert!(matches!(fake.tab.kit().log, Some(LogState::Lines(_))));
}

#[test]
fn what_gh_says_is_text_never_markup() {
    let mut hostile: serde_json::Value = serde_json::from_str(RUN_SUCCESS).expect("json");
    hostile["workflowName"] = json!("<img src=x onerror=alert(1)>");
    hostile["displayTitle"] = json!("<script>alert(1)</script>");
    let fake = Setup::new()
        .command("ci-run", move |_| ok(hostile.to_string()))
        .start();
    assert_eq!(fake.header().title, "<script>alert(1)</script>");
    assert!(
        fake.tab.shows("<img src=x onerror=alert(1)>"),
        "in the line under it, as written"
    );
    assert!(
        fake.widgets()
            .iter()
            .all(|w| !matches!(w, Element::Unsupported)),
        "there is no markup to parse"
    );
}

#[test]
fn a_run_that_is_over_has_its_age_move_by_the_minute() {
    let mut fake = Setup::new().now(NOW).serves("ci-run", RUN_SUCCESS).start();
    assert!(!fake.ticks("tick"));
    assert!(fake.ticks("age"));
    let before = fake.header().state.iter().map(said).collect::<Vec<_>>();
    fake.advance(60 * 60_000);
    assert_ne!(
        fake.header().state.iter().map(said).collect::<Vec<_>>(),
        before
    );
}

#[test]
fn no_widget_asks_for_a_colour_or_a_size() {
    // The tab names what something is (a glyph, a state, a tone): Hangar draws it.
    let fake = failed_run().start();
    let json = serde_json::to_string(&fake.tab.view().root).expect("a view");
    assert!(!json.contains('#'), "no hex colour");
    assert!(!json.contains("rgb"));
    // Nothing of the page exceeds what a view may weigh by far.
    assert!(json.len() < 100_000, "{} bytes", json.len());
    let _ = ProgressKind::Normal;
}
