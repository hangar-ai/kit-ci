//! The tab's behaviour against a fake Hangar and a fake clock: what it asks,
//! what it sets, when it polls and stops, when it notifies.
//! `test/controller.test.js`, ported, case for case, and the cases of what a
//! native tab does that the page's script did not (`tab.params` as an event,
//! the log's windows, a tab behind another).

mod support;

use std::cell::Cell;
use std::rc::Rc;

use hangar_kit_sdk::ui::abi::Operation;
use kit_ci_tab::logic::ErrorKind;
use kit_ci_tab::{LogState, POLL, Screen};
use serde_json::json;
use support::*;

/// A command that answers these in turn, then the last one for good.
fn in_turn(outputs: Vec<Said>) -> impl FnMut(&std::collections::BTreeMap<String, String>) -> Said {
    let mut outputs: std::collections::VecDeque<Said> = outputs.into();
    move |_| {
        if outputs.len() > 1 {
            outputs.pop_front().expect("one more")
        } else {
            match outputs.front().expect("an answer") {
                Said::Ran {
                    exit_code,
                    stdout,
                    stderr,
                    timed_out,
                    truncated,
                } => Said::Ran {
                    exit_code: *exit_code,
                    stdout: stdout.clone(),
                    stderr: stderr.clone(),
                    timed_out: *timed_out,
                    truncated: *truncated,
                },
                Said::Refused(code) => Said::Refused(code),
                Said::Held => Said::Held,
            }
        }
    }
}

#[test]
fn running_then_success_title_badge_polling_every_5_s_one_notification_then_silence() {
    let mut fake = Setup::new()
        .command(
            "ci-run",
            in_turn(vec![ok(RUN_RUNNING), ok(RUN_RUNNING), ok(RUN_SUCCESS)]),
        )
        .start();

    assert_eq!(fake.titles(), ["CI"]);
    assert_eq!(fake.badges(), [json!("running")]);
    assert_eq!(fake.tab.kit().screen, Screen::Run);
    assert!(fake.calls_to("notify").is_empty());
    assert_eq!(fake.runs_of("ci-run").len(), 1);
    assert_eq!(
        fake.runs_of("ci-run")[0].params["args"],
        json!({ "id": ID })
    );

    fake.advance(4999);
    assert_eq!(fake.runs_of("ci-run").len(), 1, "not before 5 s");
    fake.advance(1);
    assert_eq!(fake.runs_of("ci-run").len(), 2);
    assert_eq!(
        fake.badges(),
        [json!("running")],
        "an unchanged badge is not set again"
    );

    fake.advance(5000);
    assert_eq!(fake.runs_of("ci-run").len(), 3);
    assert_eq!(fake.badges(), [json!("running"), json!("success")]);
    let notices = fake.calls_to("notify");
    assert_eq!(notices.len(), 1, "once");
    assert_eq!(notices[0].params["title"], "CI: passed");
    assert_eq!(
        notices[0].params["body"], "Point the site's downloads to 1.8.0 · main",
        "what ran, and where"
    );
    assert!(fake.runs_of("ci-log").is_empty(), "no log for a success");

    fake.advance(10 * 60 * 1000);
    assert_eq!(fake.runs_of("ci-run").len(), 3, "polling stopped");
    assert!(!fake.polls());
    assert!(!fake.ticks("tick"), "and the clock of the jobs");
    assert_eq!(fake.calls_to("notify").len(), 1);
}

#[test]
fn a_failure_badge_failure_notification_and_the_failed_steps_log() {
    let mut fake = Setup::new()
        .lang("fr")
        .command("ci-run", in_turn(vec![ok(RUN_RUNNING), ok(RUN_FAILURE)]))
        .serves("ci-log", LOG_FAILED)
        .start();
    fake.advance(POLL.as_millis() as i64);

    assert_eq!(fake.badges(), [json!("running"), json!("failure")]);
    assert_eq!(
        fake.calls_to("notify")[0].params["title"],
        "CI\u{202f}: échouée",
        "French puts a narrow space before its colon"
    );
    assert_eq!(fake.runs_of("ci-log").len(), 1);
    assert_eq!(
        fake.runs_of("ci-log")[0].params["args"],
        json!({ "id": ID })
    );
    let Some(LogState::Lines(lines)) = &fake.tab.kit().log else {
        panic!("the log of the failed steps");
    };
    assert!(
        lines.log.lines.len() > 100,
        "the whole of it: the page windows it"
    );
    assert!(
        lines.log.lines[lines.log.errors[0]]
            .text
            .contains("panicked at")
    );
    assert!(!fake.polls());
    assert!(!fake.ticks("tick"));
}

#[test]
fn a_log_cut_at_the_commands_limit_says_so() {
    let fake = Setup::new()
        .serves("ci-run", RUN_FAILURE)
        .command("ci-log", |_| cut("a line\n"))
        .start();
    let Some(LogState::Lines(lines)) = &fake.tab.kit().log else {
        panic!("a log");
    };
    assert!(lines.log.truncated);
    assert!(
        fake.tab
            .shows("Only the first megabyte of this log is shown.")
    );
}

#[test]
fn hangars_language_changing_says_the_badge_again_in_it_and_nothing_on_a_page_without_a_run() {
    let mut fake = Setup::new().serves("ci-run", RUN_SUCCESS).start();
    fake.speak("fr");
    let texts: Vec<_> = fake
        .calls_to("tab.setBadge")
        .iter()
        .map(|call| call.params["text"].clone())
        .collect();
    assert_eq!(texts, [json!("passed"), json!("réussie")]);
    assert!(fake.tab.shows("Réussie"), "and the page in it");

    let mut list = Setup::new().params(&[]).serves("ci-list", RUN_LIST).start();
    let before = list.calls_to("tab.setBadge").len();
    list.speak("fr");
    assert_eq!(list.calls_to("tab.setBadge").len(), before);
    assert!(list.tab.shows("Exécutions récentes"));
}

#[test]
fn refreshing_the_picker_keeps_the_runs_it_knows_on_screen_and_a_failure_then_says_so() {
    let mut fake = Setup::new()
        .params(&[])
        .command("ci-list", |_| Said::Held)
        .start();
    assert_eq!(fake.tab.kit().screen, Screen::Loading, "nothing known yet");
    assert!(!fake.tab.kit().refreshing);
    fake.release("ci-list", ok(RUN_LIST));

    fake.press("Refresh");
    assert_eq!(fake.tab.kit().screen, Screen::Picker, "the rows stay");
    assert!(fake.tab.kit().refreshing);
    assert_eq!(fake.tab.kit().runs.as_ref().map(Vec::len), Some(15));
    fake.release("ci-list", ok(RUN_LIST));
    assert!(!fake.tab.kit().refreshing);

    fake.press("Refresh");
    fake.release("ci-list", fails(4, NOT_LOGGED_IN));
    assert_eq!(fake.tab.kit().screen, Screen::Error);
    assert!(!fake.tab.kit().refreshing);
    assert_eq!(
        fake.tab.kit().error.as_ref().map(|e| e.kind),
        Some(ErrorKind::NoAuth)
    );
}

#[test]
fn a_run_already_over_when_opened_its_result_no_notification_no_polling() {
    let mut fake = Setup::new().serves("ci-run", RUN_SUCCESS).start();
    fake.advance(60_000);
    assert_eq!(fake.badges(), [json!("success")]);
    assert!(fake.calls_to("notify").is_empty());
    assert_eq!(fake.runs_of("ci-run").len(), 1);
}

#[test]
fn a_notification_the_kit_may_not_raise_does_not_break_the_tab() {
    let mut fake = Setup::new()
        .deny_notify()
        .command("ci-run", in_turn(vec![ok(RUN_RUNNING), ok(RUN_FAILURE)]))
        .serves("ci-log", LOG_FAILED)
        .start();
    fake.advance(POLL.as_millis() as i64);
    assert_eq!(fake.tab.kit().screen, Screen::Run);
    assert!(fake.tab.kit().log.is_some());
    assert_eq!(fake.calls_to("notify").len(), 1);
}

#[test]
fn a_log_that_cannot_be_read_leaves_the_result_standing() {
    let fake = Setup::new()
        .serves("ci-run", RUN_FAILURE)
        .command("ci-log", |_| fails(1, "run 1 is still in progress"))
        .start();
    assert_eq!(fake.tab.kit().screen, Screen::Run);
    assert_eq!(fake.tab.kit().log, Some(LogState::Failed(ErrorKind::Error)));
    assert_eq!(fake.badges(), [json!("failure")]);
}

#[test]
fn gh_not_logged_in_says_so_takes_the_badge_off_does_not_poll_retries_on_request() {
    let signed_in = Rc::new(Cell::new(false));
    let seen = signed_in.clone();
    let mut fake = Setup::new()
        .command("ci-run", move |_| {
            if seen.get() {
                ok(RUN_SUCCESS)
            } else {
                fails(4, NOT_LOGGED_IN)
            }
        })
        .start();
    fake.advance(60_000);
    assert_eq!(fake.tab.kit().screen, Screen::Error);
    assert_eq!(
        fake.tab.kit().error.as_ref().map(|e| e.kind),
        Some(ErrorKind::NoAuth)
    );
    assert_eq!(fake.runs_of("ci-run").len(), 1);
    assert_eq!(fake.badges(), [json!(null)]);

    signed_in.set(true);
    fake.press("Try again");
    assert_eq!(fake.tab.kit().screen, Screen::Run);
    assert_eq!(fake.badges(), [json!(null), json!("success")]);
}

#[test]
fn gh_absent_not_found_is_explained_nothing_throws() {
    let fake = Setup::new()
        .command("ci-run", |_| Said::Refused("not_found"))
        .start();
    assert_eq!(
        fake.tab.kit().error.as_ref().map(|e| e.kind),
        Some(ErrorKind::NoGh)
    );
    assert!(fake.calls_to("notify").is_empty());
}

#[test]
fn the_kits_commands_not_accepted_denied_is_explained() {
    let fake = Setup::new().start();
    assert_eq!(
        fake.tab.kit().error.as_ref().map(|e| e.kind),
        Some(ErrorKind::Denied)
    );
}

#[test]
fn a_hiccup_while_following_a_run_is_retried_three_in_a_row_end_the_vigil() {
    let mut fake = Setup::new()
        .command(
            "ci-run",
            in_turn(vec![
                ok(RUN_RUNNING),
                fails(1, "error connecting to api.github.com"),
            ]),
        )
        .start();
    fake.advance(POLL.as_millis() as i64);
    assert_eq!(fake.tab.kit().screen, Screen::Run);
    assert!(fake.tab.kit().warning);
    fake.advance(POLL.as_millis() as i64);
    assert_eq!(fake.tab.kit().screen, Screen::Run);
    fake.advance(POLL.as_millis() as i64);
    assert_eq!(fake.tab.kit().screen, Screen::Error);
    assert_eq!(fake.runs_of("ci-run").len(), 4);
    assert!(!fake.polls());
    assert!(!fake.ticks("tick"));
}

#[test]
fn a_hiccup_that_passes_clears_the_warning() {
    let mut fake = Setup::new()
        .command(
            "ci-run",
            in_turn(vec![ok(RUN_RUNNING), fails(1, "timeout"), ok(RUN_RUNNING)]),
        )
        .start();
    fake.advance(POLL.as_millis() as i64);
    assert!(fake.tab.kit().warning);
    fake.advance(POLL.as_millis() as i64);
    assert!(!fake.tab.kit().warning);
}

#[test]
fn without_an_id_a_picker_of_recent_runs_and_choosing_one_follows_it() {
    let mut fake = Setup::new()
        .params(&[])
        .serves("ci-list", RUN_LIST)
        .serves("ci-run", RUN_SUCCESS)
        .start();
    assert_eq!(fake.tab.kit().screen, Screen::Picker);
    assert_eq!(fake.tab.kit().runs.as_ref().map(Vec::len), Some(15));
    assert!(fake.runs_of("ci-run").is_empty());
    assert_eq!(fake.titles(), ["CI"]);

    fake.press("Point the site's downloads to 1.8.0");
    assert_eq!(fake.tab.kit().screen, Screen::Run);
    assert!(fake.tab.kit().from_picker);
    assert_eq!(
        fake.runs_of("ci-run")[0].params["args"],
        json!({ "id": "41231065926" })
    );

    fake.press("Back to the recent runs");
    assert_eq!(fake.tab.kit().screen, Screen::Picker);
    assert_eq!(fake.titles().last().map(String::as_str), Some("CI"));
    assert!(!fake.polls());
    assert!(!fake.ticks("tick"));
}

#[test]
fn an_id_that_is_not_digits_is_no_id_the_picker() {
    for id in ["abc", "-1", "12 34", ""] {
        let fake = Setup::new()
            .params(&[("id", id)])
            .serves("ci-list", RUN_LIST)
            .start();
        assert_eq!(fake.tab.kit().screen, Screen::Picker, "{id:?}");
        assert!(fake.runs_of("ci-run").is_empty(), "{id:?}");
    }
}

#[test]
fn the_picker_without_gh_logged_in_says_so() {
    let fake = Setup::new()
        .params(&[])
        .command("ci-list", |_| fails(4, NOT_LOGGED_IN))
        .start();
    assert_eq!(fake.tab.kit().screen, Screen::Error);
    assert_eq!(
        fake.tab.kit().error.as_ref().map(|e| e.kind),
        Some(ErrorKind::NoAuth)
    );
}

#[test]
fn leaving_a_run_mid_poll_drops_its_answers() {
    let mut fake = Setup::new()
        .params(&[])
        .serves("ci-list", RUN_LIST)
        .command("ci-run", |_| Said::Held)
        .start();
    fake.press("Point the site's downloads to 1.8.0");
    assert!(fake.holds("ci-run"));
    fake.press("Back to the recent runs");
    fake.release("ci-run", ok(RUN_RUNNING));
    fake.advance(1000);
    assert_eq!(fake.tab.kit().screen, Screen::Picker);
    assert!(fake.tab.kit().run.is_none());
    assert!(!fake.polls());
    assert!(!fake.ticks("tick"));
    assert_eq!(fake.badges(), [json!(null)], "the stale run set no badge");
}

// ------------------------------------------------ What the page did not do.

#[test]
fn the_tab_is_asked_for_again_with_another_run_and_follows_that_one() {
    let mut fake = Setup::new()
        .serves("ci-run", RUN_RUNNING)
        .serves("ci-list", RUN_LIST)
        .start();
    assert_eq!(fake.runs_of("ci-run").len(), 1);
    fake.event(
        "tab.params",
        json!({ "params": { "id": "77" }, "caller": null }),
    );
    let runs = fake.runs_of("ci-run");
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].params["args"], json!({ "id": "77" }));
    assert_eq!(fake.tab.kit().id.as_deref(), Some("77"));
    // The old poll is gone: one run is followed at a time.
    fake.advance(POLL.as_millis() as i64);
    assert_eq!(fake.runs_of("ci-run").len(), 3);
    assert!(fake.runs_of("ci-run")[2].params["args"] == json!({ "id": "77" }));

    fake.event("tab.params", json!({ "params": {}, "caller": null }));
    assert_eq!(fake.tab.kit().screen, Screen::Picker);
    assert_eq!(fake.tab.kit().id, None);
    assert_eq!(fake.runs_of("ci-list").len(), 1);
    fake.advance(POLL.as_millis() as i64);
    assert_eq!(
        fake.runs_of("ci-run").len(),
        3,
        "and the poll of the run left is gone"
    );
}

#[test]
fn behind_another_tab_nothing_ticks_but_the_poll_goes_on() {
    let mut fake = Setup::new()
        .command("ci-run", in_turn(vec![ok(RUN_RUNNING), ok(RUN_SUCCESS)]))
        .start();
    assert!(fake.ticks("tick"));
    fake.event("tab.visibility", json!({ "visible": false }));
    assert!(!fake.ticks("tick"));
    assert!(!fake.ticks("age"));
    fake.advance(POLL.as_millis() as i64);
    assert_eq!(
        fake.runs_of("ci-run").len(),
        2,
        "the badge and the notice are for a tab nobody looks at"
    );
    assert_eq!(fake.calls_to("notify").len(), 1);
    fake.event("tab.visibility", json!({ "visible": true }));
    assert!(
        fake.ticks("age"),
        "a run that is over: its age moves by the minute"
    );
}

#[test]
fn the_clock_runs_while_a_run_goes_and_the_page_follows_it() {
    let mut fake = Setup::new().serves("ci-run", RUN_RUNNING).start();
    let first = fake.tab.kit().now;
    fake.advance(3000);
    assert_eq!(fake.tab.kit().now, first + 3000);
}

#[test]
fn the_log_opens_on_its_first_error_and_the_bar_moves_between_them() {
    let mut fake = Setup::new()
        .serves("ci-run", RUN_FAILURE)
        .serves("ci-log", LOG_FAILED)
        .start();
    let id = fake.tab.kit().log_id();
    let errors = {
        let Some(LogState::Lines(lines)) = &fake.tab.kit().log else {
            panic!("a log");
        };
        assert_eq!(lines.current, 0);
        lines.log.errors.clone()
    };
    assert_eq!(errors.len(), 5);
    let scrolled = |line: usize| Operation::ScrollTo {
        id: id.clone(),
        line: Some(line as u32),
        y: None,
        end: false,
    };
    assert_eq!(fake.tab.take_operations(), [scrolled(errors[0])]);
    assert!(fake.tab.shows("Error 1 of 5"));

    fake.press("Next");
    assert_eq!(fake.tab.take_operations(), [scrolled(errors[1])]);
    assert!(fake.tab.shows("Error 2 of 5"));
    fake.press("Previous");
    fake.press("Previous");
    assert_eq!(
        fake.tab.take_operations(),
        [scrolled(errors[0]), scrolled(errors[4])],
        "round the end"
    );
    assert!(fake.tab.shows("Error 5 of 5"));
    fake.press("Next");
    assert_eq!(fake.tab.take_operations(), [scrolled(errors[0])]);
}

#[test]
fn the_log_is_given_in_parts_and_asks_for_the_others_as_they_scroll() {
    let mut fake = Setup::new()
        .serves("ci-run", RUN_FAILURE)
        .serves("ci-log", LOG_FAILED)
        .start();
    let id = fake.tab.kit().log_id();
    let total = {
        let Some(LogState::Lines(lines)) = &fake.tab.kit().log else {
            panic!("a log");
        };
        assert!(
            lines.window.1 - lines.window.0 <= 400,
            "a part, not the whole"
        );
        lines.log.lines.len()
    };
    assert_eq!(total, 151);
    fake.tab.range(&id, 0, 120);
    let Some(LogState::Lines(lines)) = &fake.tab.kit().log else {
        panic!("a log");
    };
    assert_eq!(lines.window, (0, 120));
    fake.tab.range(&id, 100, 5000);
    let Some(LogState::Lines(lines)) = &fake.tab.kit().log else {
        panic!("a log");
    };
    assert_eq!(lines.window, (100, total), "never past its end");
    fake.tab.range(&id, 9000, 9500);
    let Some(LogState::Lines(lines)) = &fake.tab.kit().log else {
        panic!("a log");
    };
    assert_eq!(
        lines.window.0, lines.window.1,
        "past its end: nothing, and no panic"
    );
}

#[test]
fn a_log_without_an_error_opens_at_its_end() {
    let fake = Setup::new()
        .serves("ci-run", RUN_FAILURE)
        .serves("ci-log", "build\tTests\t2026-10-31T17:02:37.1Z all fine\n")
        .start();
    let Some(LogState::Lines(lines)) = &fake.tab.kit().log else {
        panic!("a log");
    };
    assert!(lines.log.errors.is_empty());
    assert!(!fake.tab.shows("error"), "and no bar of errors");
}

#[test]
fn the_link_and_the_commands_to_copy_go_to_the_clipboard_with_a_word_of_thanks() {
    let mut fake = Setup::new()
        .serves("ci-run", RUN_FAILURE)
        .serves("ci-log", LOG_FAILED)
        .start();
    fake.press("Copy the link");
    let copies = fake.calls_to("ui.copy");
    assert_eq!(copies.len(), 1);
    assert_eq!(
        copies[0].params["text"],
        "https://github.com/hangar-ai/hangar/actions/runs/41222364482"
    );
    let toasts = fake.calls_to("ui.toast");
    assert_eq!(toasts.len(), 1);
    assert_eq!(toasts[0].params["message"], "Copied");

    let mut signed_out = Setup::new()
        .command("ci-run", |_| fails(4, NOT_LOGGED_IN))
        .start();
    signed_out.press("Copy");
    assert_eq!(
        signed_out.calls_to("ui.copy")[0].params["text"],
        "gh auth login"
    );
}

#[test]
fn a_reply_that_is_not_a_run_is_shown_not_trusted() {
    let fake = Setup::new()
        .serves("ci-run", "<html>proxy login</html>")
        .start();
    let error = fake.tab.kit().error.clone().expect("an error");
    assert_eq!(error.kind, ErrorKind::Error);
    assert_eq!(error.detail, "<html>proxy login</html>");
    assert!(fake.tab.shows("gh failed"));
}

#[test]
fn what_a_command_said_is_cut_to_500_characters() {
    let long = "é".repeat(900);
    let fake = Setup::new()
        .command("ci-run", move |_| {
            fails(1, format!("\u{1b}[31m{long}\u{1b}[0m\n"))
        })
        .start();
    assert_eq!(
        fake.tab
            .kit()
            .error
            .as_ref()
            .map(|e| e.detail.chars().count()),
        Some(500)
    );
}

#[test]
fn a_command_that_timed_out_says_gh_took_too_long() {
    let fake = Setup::new().command("ci-run", |_| timed_out()).start();
    assert_eq!(
        fake.tab.kit().error.as_ref().map(|e| e.kind),
        Some(ErrorKind::Timeout)
    );
}

#[test]
fn the_calls_are_the_kits_declared_commands_and_nothing_else() {
    let mut fake = Setup::new()
        .command("ci-run", in_turn(vec![ok(RUN_RUNNING), ok(RUN_FAILURE)]))
        .serves("ci-log", LOG_FAILED)
        .start();
    fake.advance(POLL.as_millis() as i64);
    let mut methods = Vec::new();
    for call in &fake.history {
        methods.push(call.method.clone());
        if call.method == "command.run" {
            let name = call.params["name"].as_str().expect("a name");
            assert!(["ci-run", "ci-list", "ci-log"].contains(&name), "{name}");
            assert_eq!(call.params["args"], json!({ "id": ID }));
        }
    }
    methods.sort();
    methods.dedup();
    assert_eq!(
        methods,
        ["command.run", "notify", "tab.setBadge", "tab.setTitle"],
        "the host calls of the page, and no more"
    );
}

/// About a megabyte: 15 000 lines of a test run, two of them failures.
fn big_log() -> String {
    let mut raw = String::new();
    for i in 0..15_000 {
        let stamp = format!("2026-10-31T17:02:37.{i:07}Z");
        if i == 9_000 || i == 14_990 {
            raw += &format!(
                "build\tTests\t{stamp} thread 'suite::case_{i}' panicked at src/lib.rs:{i}:9:\n"
            );
        } else {
            raw += &format!("build\tTests\t{stamp} test suite::module::case_number_{i} ... ok\n");
        }
    }
    raw
}

#[test]
fn a_megabyte_of_log_is_read_whole_and_given_in_parts() {
    let raw = big_log();
    assert!(raw.len() > 1_000_000, "{} bytes", raw.len());
    let for_command = raw.clone();
    let mut fake = Setup::new()
        .serves("ci-run", RUN_FAILURE)
        .command("ci-log", move |_| cut(for_command.clone()))
        .start();
    let id = fake.tab.kit().log_id();
    let Some(LogState::Lines(lines)) = &fake.tab.kit().log else {
        panic!("a log");
    };
    assert_eq!(lines.log.lines.len(), 15_001, "each line, and a heading");
    assert_eq!(lines.log.errors, [9_001, 14_991]);
    assert!(
        lines.window.1 - lines.window.0 <= 400,
        "a part: {:?}",
        lines.window
    );
    assert!(
        lines.window.0 <= 9_001 && 9_001 < lines.window.1,
        "the one with its first error"
    );

    // What Hangar is sent each time the page is drawn stays small.
    let view = serde_json::to_string(&fake.tab.view().root).expect("a view");
    assert!(view.len() < 150_000, "{} bytes of view", view.len());

    // It scrolls on: Hangar asks, and gets the lines it asked for.
    fake.tab.range(&id, 14_700, 15_001);
    let log = fake.log().expect("a log");
    assert_eq!((log.first, log.lines.len()), (14_700, 301));
    assert_eq!(log.total, Some(15_001));
    assert!(
        log.lines
            .iter()
            .any(|l| l.kind == hangar_kit_sdk::ui::LineKind::Error)
    );
    fake.press("Next");
    assert!(fake.tab.shows("Error 2 of 2"));
}

#[test]
fn reading_a_megabyte_of_log_takes_a_few_milliseconds_not_the_250_a_tab_is_given() {
    let raw = big_log();
    let started = std::time::Instant::now();
    let log = kit_ci_tab::logic::read_log(&raw, false);
    let took = started.elapsed();
    assert_eq!(log.lines.len(), 15_001);
    // A debug build is no measure; the release one, which is the module's, is.
    if !cfg!(debug_assertions) {
        assert!(took.as_millis() < 100, "{took:?}");
    }
}
