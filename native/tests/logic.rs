//! The tab's logic on recorded gh output. (fixtures/*.json come from `gh run view/list` on this very
//! repository; run-running.json is the success run with its status turned to
//! "in progress", as gh writes it.)

mod support;

use std::collections::BTreeMap;

use hangar_kit_sdk::api::BadgeState;
use kit_ci_tab::logic::*;
use kit_ci_tab::strings::{EN, FR, strings};
use support::*;

fn run(json: &str) -> Run {
    Run::parse(json).expect("a run")
}

fn success() -> Run {
    run(RUN_SUCCESS)
}
fn failure() -> Run {
    run(RUN_FAILURE)
}
fn running() -> Run {
    run(RUN_RUNNING)
}

fn plain(status: &str, conclusion: &str) -> Run {
    Run {
        status: status.to_owned(),
        conclusion: conclusion.to_owned(),
        ..Run::default()
    }
}

/// The word a badge says, in a language.
fn said(locale: &str, badge: Badge) -> String {
    strings(locale).t(badge.key)
}

fn holes(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close)
                if close > 0
                    && after[..close]
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_') =>
            {
                found.push(after[..close].to_owned());
                rest = &after[close + 1..];
            }
            _ => rest = after,
        }
    }
    found.sort();
    found
}

#[test]
fn every_french_string_exists_with_its_placeholders_and_carries_accents_where_french_needs_them() {
    let english: BTreeMap<_, _> = EN.iter().copied().collect();
    let french: BTreeMap<_, _> = FR.iter().copied().collect();
    assert_eq!(english.len(), EN.len(), "a key twice in English");
    assert_eq!(french.len(), FR.len(), "a key twice in French");
    assert_eq!(
        french.keys().collect::<Vec<_>>(),
        english.keys().collect::<Vec<_>>()
    );
    for (key, text) in &english {
        assert_eq!(holes(french[key]), holes(text), "{key}");
    }
    assert!(french["failure"].contains('É'));
    assert!(french["err.no-auth.title"].contains("n'est pas connecté"));
}

/// `\b(?:vous|votre|vos)\b|[a-zéè]ez\b`: the words of « vous ».
fn speaks_to_vous(text: &str) -> bool {
    text.split(|c: char| !c.is_alphabetic()).any(|word| {
        let word = word.to_lowercase();
        let mut letters = word.chars().rev();
        let ends_in_ez = letters.next() == Some('z')
            && letters.next() == Some('e')
            && letters
                .next()
                .is_some_and(|c| c.is_ascii_lowercase() || "éè".contains(c));
        matches!(word.as_str(), "vous" | "votre" | "vos") || ends_in_ez
    })
}

#[test]
fn french_speaks_as_hangar_does_tu_workspace_reglages() {
    for (key, text) in FR {
        assert!(!speaks_to_vous(text), "{key}: « tu »");
        assert!(
            !text.to_lowercase().contains("espace de travail"),
            "{key}: « workspace »"
        );
        assert!(!text.contains("Paramètres"), "{key}: « Réglages »");
        assert!(
            !text.contains('’'),
            "{key}: the apostrophe Hangar's strings use"
        );
    }
    let fr: BTreeMap<_, _> = FR.iter().copied().collect();
    assert!(fr["err.denied.hint"].contains("Réglages › Kits"));
    assert!(speaks_to_vous("Réessayez dans un instant"));
}

#[test]
fn a_running_run_badge_running_jobs_with_progress_and_the_step_going() {
    let running = running();
    let now = running.started_at.expect("started") + 104_000;
    let view = run_view(&running, now);
    assert_eq!(view.kind, Kind::Running);
    assert!(!view.done);
    assert_eq!(view.ended_at, None);
    assert_eq!(view.name, "CI");
    let badge = badge_for(&running);
    assert_eq!(badge.state, BadgeState::Running);
    assert_eq!(said("en", badge), "running");
    assert_eq!(said("fr", badge), "en cours");

    let going = view
        .jobs
        .iter()
        .find(|job| job.kind == Kind::Running)
        .expect("a job going");
    assert_eq!(going.name, "rust (macos-latest)");
    assert_eq!(going.at, 3, "the step it is at, two being done");
    assert_eq!(going.total, 13);
    assert_eq!(
        going.step.as_deref(),
        Some("Keep Defender out of the build")
    );
    assert_eq!(going.failed_step, None);
    assert_eq!(
        view.jobs
            .iter()
            .filter(|job| job.kind == Kind::Queued)
            .count(),
        2
    );
    // Time elapsed runs on with the clock until the run ends.
    assert_eq!(view.elapsed, Some(104_000));
    assert_eq!(
        run_view(&running, now + 16_000).elapsed,
        Some(104_000 + 16_000)
    );
}

#[test]
fn how_long_a_job_took_nothing_before_it_starts_the_clock_while_it_runs_its_own_once_over() {
    let running = running();
    let now = running.started_at.expect("started") + 104_000;
    let jobs = run_view(&running, now).jobs;
    let going = jobs
        .iter()
        .find(|job| job.kind == Kind::Running)
        .expect("going");
    let queued = jobs
        .iter()
        .find(|job| job.kind == Kind::Queued)
        .expect("queued");
    assert_eq!(job_duration(queued, now), None);
    assert_eq!(
        job_duration(going, now + 1000),
        job_duration(going, now).map(|ms| ms + 1000)
    );
    let view = run_view(&failure(), now);
    let over = view.failed_jobs()[0];
    let took = at("2026-10-31T17:02:43Z") - at("2026-10-31T17:00:20Z");
    assert_eq!(job_duration(over, now), Some(took));
    assert_eq!(
        job_duration(over, now + 60_000),
        job_duration(over, now),
        "a job over no longer counts"
    );
}

#[test]
fn a_successful_run_badge_success_time_elapsed_is_the_runs_no_failed_job() {
    let success = success();
    let view = run_view(&success, now_ms());
    assert_eq!(view.kind, Kind::Success);
    assert!(view.done);
    assert_eq!(view.ended_at, success.updated_at);
    assert!(view.failed_jobs().is_empty());
    assert_eq!(view.other_jobs().len(), view.jobs.len());
    let badge = badge_for(&success);
    assert_eq!(badge.state, BadgeState::Success);
    assert_eq!(said("en", badge), "passed");
    assert_eq!(said("fr", badge), "réussie");
    let first = success
        .jobs
        .iter()
        .filter_map(|job| job.started_at)
        .min()
        .expect("a job");
    let last = success
        .jobs
        .iter()
        .filter_map(|job| job.completed_at)
        .chain(success.updated_at)
        .max()
        .expect("an end");
    assert_eq!(
        view.elapsed,
        Some(last - first.min(success.started_at.expect("started")))
    );
    assert_eq!(
        run_view(&success, 0).elapsed,
        view.elapsed,
        "a finished run no longer counts"
    );
}

/// Any moment will do for a run that is over.
fn now_ms() -> i64 {
    at("2026-11-03T12:00:00Z")
}

#[test]
fn a_failed_run_badge_failure_the_failed_job_and_its_failed_step_found() {
    let failure = failure();
    let view = run_view(&failure, now_ms());
    assert_eq!(view.kind, Kind::Failure);
    let names: Vec<_> = view
        .failed_jobs()
        .iter()
        .map(|job| job.name.clone())
        .collect();
    assert_eq!(names, ["rust (macos-latest)"]);
    assert_eq!(
        view.failed_jobs()[0].failed_step.as_deref(),
        Some("Tests"),
        "not the steps skipped around it"
    );
    assert_eq!(view.other_jobs().len(), 3);
    let badge = badge_for(&failure);
    assert_eq!(badge.state, BadgeState::Failure);
    assert_eq!(said("en", badge), "failed");
    assert_eq!(view.elapsed, Some(256_000));
}

#[test]
fn conclusions_that_are_neither_a_pass_nor_a_failure_are_neutral() {
    for conclusion in [
        "cancelled",
        "skipped",
        "neutral",
        "stale",
        "action_required",
        "",
    ] {
        assert_eq!(
            badge_for(&plain("completed", conclusion)).state,
            BadgeState::Neutral,
            "{conclusion}"
        );
    }
    for conclusion in ["failure", "timed_out", "startup_failure"] {
        assert_eq!(
            badge_for(&plain("completed", conclusion)).state,
            BadgeState::Failure,
            "{conclusion}"
        );
    }
    assert_eq!(badge_for(&plain("queued", "")).state, BadgeState::Running);
    assert_eq!(said("en", badge_for(&plain("waiting", ""))), "queued");
}

#[test]
fn the_word_for_a_state_says_what_a_neutral_end_was_when_it_is_known() {
    assert_eq!(word_of("completed", "cancelled"), Word::Cancelled);
    assert_eq!(word_of("completed", "skipped"), Word::Skipped);
    assert_eq!(word_of("completed", "stale"), Word::Neutral);
    assert_eq!(word_of("completed", "failure"), Word::Failure);
    assert_eq!(word_of("in_progress", ""), Word::Running);
    let (en, fr) = (strings("en"), strings("fr"));
    for word in [
        Word::Cancelled,
        Word::Skipped,
        Word::Neutral,
        Word::Queued,
        Word::Running,
        Word::Success,
        Word::Failure,
    ] {
        for key in [word.key(), word.job_key()] {
            assert_ne!(en.t(key), key);
            assert_ne!(fr.t(key), key);
        }
    }
    // In French a run is feminine and a job is not.
    assert_eq!(fr.t("cancelled"), "Annulée");
    assert_eq!(fr.t(Word::Cancelled.job_key()), "Annulé");
    assert_eq!(fr.t(Word::Failure.job_key()), "Échoué");
}

#[test]
fn badge_text_fits_tab_set_badge_12_characters_and_titles_tab_set_title_40() {
    for locale in ["en", "fr"] {
        let t = strings(locale);
        for key in [
            "badgeRunning",
            "badgeQueued",
            "badgeSuccess",
            "badgeFailure",
            "badgeNeutral",
        ] {
            assert!(t.t(key).chars().count() <= 12, "{key}");
        }
    }
    let title = one_line(&format!("{}\n", "x".repeat(100)), 40);
    assert_eq!(title.chars().count(), 40);
    assert_eq!(one_line("  a\n\tb\u{202e} c ", 40), "a b c");
    assert_eq!(one_line("été", 3), "été");
    assert_eq!(
        one_line("été !", 3),
        "ét…",
        "by characters, never in the middle of one"
    );
}

#[test]
fn a_job_not_started_has_no_start_githubs_0001_01_01_is_none() {
    let queued = Run {
        status: "queued".to_owned(),
        jobs: vec![Job {
            started_at: time("0001-01-01T00:00:00Z"),
            ..Job::default()
        }],
        ..Run::default()
    };
    assert_eq!(elapsed_ms(&queued, now_ms()), None);
    assert_eq!(time("0001-01-01T00:00:00Z"), None);
    assert_eq!(
        time("2026-10-31T17:02:00Z"),
        Some(at("2026-10-31T17:02:00Z"))
    );
    assert_eq!(time("not a date"), None);
    let parsed = Run::parse(
        r#"{"status":"queued","jobs":[{"status":"queued","startedAt":"0001-01-01T00:00:00Z"}]}"#,
    )
    .expect("a run");
    assert_eq!(job_view(&parsed.jobs[0]).started_at, None);
}

#[test]
fn not_logged_in_no_gh_no_repository_no_such_run_told_apart() {
    let ended = |exit_code, stderr| Fault::Ended {
        exit_code: Some(exit_code),
        stderr,
        timed_out: false,
    };
    assert_eq!(classify(ended(4, NOT_LOGGED_IN)), ErrorKind::NoAuth);
    assert_eq!(
        classify(Fault::Rejected { code: "not_found" }),
        ErrorKind::NoGh
    );
    assert_eq!(
        classify(ended(
            1,
            "fatal: not a git repository (or any of the parent directories): .git"
        )),
        ErrorKind::NoRepo
    );
    assert_eq!(
        classify(ended(1, "failed to run git: no git remotes found")),
        ErrorKind::NoRepo
    );
    assert_eq!(
        classify(ended(
            1,
            "could not resolve to a Run with the databaseId of 1: HTTP 404"
        )),
        ErrorKind::RunNotFound
    );
    assert_eq!(
        classify(Fault::Rejected { code: "denied" }),
        ErrorKind::Denied
    );
    assert_eq!(
        classify(Fault::Rejected {
            code: "unavailable"
        }),
        ErrorKind::Unavailable
    );
    assert_eq!(
        classify(Fault::Ended {
            exit_code: None,
            stderr: "",
            timed_out: true
        }),
        ErrorKind::Timeout
    );
    assert_eq!(
        classify(Fault::Ended {
            exit_code: None,
            stderr: "",
            timed_out: false
        }),
        ErrorKind::Timeout,
        "a command that did not end by itself"
    );
    assert_eq!(classify(ended(1, "something odd")), ErrorKind::Error);
    assert_eq!(
        classify(Fault::Rejected {
            code: "rate_limited"
        }),
        ErrorKind::Error
    );
    assert!(
        ErrorKind::NoAuth.is_fatal()
            && !ErrorKind::Timeout.is_fatal()
            && !ErrorKind::Error.is_fatal()
    );
    // Each failure has words in both languages.
    for kind in [
        ErrorKind::NoGh,
        ErrorKind::NoAuth,
        ErrorKind::NoRepo,
        ErrorKind::RunNotFound,
        ErrorKind::Denied,
        ErrorKind::Unavailable,
        ErrorKind::Timeout,
        ErrorKind::Error,
    ] {
        for locale in ["en", "fr"] {
            let t = strings(locale);
            for part in ["title", "hint"] {
                let key = format!("err.{}.{part}", kind.key());
                assert_ne!(t.t(&key), key);
            }
        }
    }
    // And what can be done about the two a person can fix, to copy.
    assert_eq!(ErrorKind::NoAuth.to_copy(), Some("gh auth login"));
    assert_eq!(ErrorKind::NoGh.to_copy(), Some("https://cli.github.com"));
    assert!(strings("fr").t("err.no-gh.hint").contains("gh auth login"));
}

#[test]
fn colour_codes_in_what_gh_said_do_not_hide_what_it_meant() {
    let coloured = "\u{1b}[31;1mgh auth login\u{1b}[0m is needed";
    assert_eq!(strip_ansi(coloured), "gh auth login is needed");
    assert_eq!(strip_ansi("no code"), "no code");
    assert_eq!(strip_ansi("\u{1b}[?25l\u{1b}[2Kline\u{1b}["), "line\u{1b}[");
    assert_eq!(strip_ansi("é\u{1b}[1mà"), "éà");
    assert_eq!(
        classify(Fault::Ended {
            exit_code: Some(4),
            stderr: "\u{1b}[1mnot logged\u{1b}[0m in\n",
            timed_out: false
        }),
        ErrorKind::NoAuth,
        "a code in the middle of a phrase does not break it"
    );
    assert_eq!(
        classify(Fault::Ended {
            exit_code: Some(4),
            stderr: "\u{1b}[1mNOT LOGGED IN\u{1b}[0m",
            timed_out: false
        }),
        ErrorKind::NoAuth
    );
}

#[test]
fn the_first_error_line_of_a_real_failed_log_is_the_panic_not_one_of_the_ok_above_it() {
    let log = read_log(LOG_FAILED, false);
    let first = &log.lines[log.errors[0]];
    assert_eq!(first.kind, LineKind::Error);
    assert_eq!(
        first.text,
        "thread 'accounts::tests::an_added_account_is_listed_after_main_and_logged_out' (18252) panicked at src/accounts.rs:688:9:"
    );
    // Everything before it is the list it was buried under, and what names the failure.
    let before = &log.lines[..log.errors[0]];
    assert_eq!(before[0].kind, LineKind::Head);
    assert!(
        before
            .iter()
            .filter(|line| line.text.ends_with(" ... ok"))
            .count()
            > 125
    );
    assert!(before.iter().all(|line| line.kind != LineKind::Error));
    // A few lines of context above it say which test this is.
    assert_eq!(
        log.lines[log.errors[0] - 2].text,
        "---- accounts::tests::an_added_account_is_listed_after_main_and_logged_out stdout ----"
    );
}

#[test]
fn every_line_of_that_log_that_says_what_went_wrong_is_marked_and_only_those() {
    let log = read_log(LOG_FAILED, false);
    let marked: Vec<&str> = log
        .errors
        .iter()
        .map(|i| log.lines[*i].text.as_str())
        .collect();
    assert_eq!(
        marked,
        [
            "thread 'accounts::tests::an_added_account_is_listed_after_main_and_logged_out' (18252) panicked at src/accounts.rs:688:9:",
            "assertion `left == right` failed",
            "error: test failed, to rerun pass `--lib`",
            "test result: FAILED. 502 passed; 1 failed; 2 ignored; 0 measured; 0 filtered out; finished in 45.41s",
            "Process completed with exit code 101.",
        ]
    );
    for index in &log.errors {
        assert_eq!(log.lines[*index].kind, LineKind::Error);
    }
    assert_eq!(
        log.lines
            .iter()
            .filter(|line| line.kind == LineKind::Error)
            .count(),
        log.errors.len()
    );
}

#[test]
fn what_reads_as_an_error_in_other_tools_logs_and_what_only_mentions_one() {
    for line in [
        "error[E0599]: no method named `x`",
        "fatal: not a git repository",
        "test remote::thing ... FAILED",
        "FAIL  src/lib/kitTabs.test.ts > a test",
        "--- FAIL: TestThing (0.00s)",
        "FAILED tests/test_a.py::test_b - assert 1 == 2",
        "npm ERR! code ELIFECYCLE",
        "TypeError: x is not a function",
        "java.io.IOException: gone",
    ] {
        assert!(is_error_line(line), "{line}");
    }
    for line in [
        "test remote::tailscale::tests::a_missing_cli_is_an_error_to_run_not_a_panic ... ok",
        "test error: handled ... ok",
        "0 errors",
        "Compiling errors-kit v1.0.0",
        "no errors found",
        "handles_failure_mode",
        "failures:",
    ] {
        assert!(!is_error_line(line), "{line}");
    }
}

/// `tests/golden/error-lines.json`: 1 100 lines, and what the kit's earlier
/// HTML tab said of each (its `isErrorLine`), written once by running that
/// function over the fixture's lines and over lines made of the words a CI
/// log uses, in every case and place. The classifier here is written by hand
/// rather than as a regular expression, so it is held to those answers.
#[test]
fn the_hand_written_classifier_agrees_with_the_pages_on_1100_lines() {
    let golden: Vec<(String, bool)> =
        serde_json::from_str(include_str!("golden/error-lines.json")).expect("the golden lines");
    assert!(golden.len() >= 1100);
    assert!(golden.iter().filter(|(_, error)| *error).count() > 200);
    let wrong: Vec<_> = golden
        .iter()
        .filter(|(line, error)| is_error_line(line) != *error)
        .map(|(line, error)| format!("{line:?}: the page says {error}"))
        .collect();
    assert!(
        wrong.is_empty(),
        "{} lines differ:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn the_failed_log_reads_as_text_no_colour_no_timestamp_no_mark_of_githubs() {
    let log = read_log(LOG_FAILED, false);
    assert_eq!(log.lines[0].kind, LineKind::Head);
    assert_eq!(log.lines[0].text, "rust (macos-latest) › Tests");
    assert_eq!(
        log.lines
            .iter()
            .filter(|line| line.kind == LineKind::Head)
            .count(),
        1
    );
    let text = log
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !text.contains('\u{1b}') && !text.contains("[36;1m"),
        "no colour codes"
    );
    assert!(!text.contains("T17:0"), "no timestamps");
    assert!(!text.contains("##[error]"));
    assert!(!log.truncated);
    assert!(read_log("a\n", true).truncated);
}

#[test]
fn nothing_of_a_log_is_cut_but_a_line_beyond_reason() {
    let log = read_log(LOG_FAILED, false);
    assert_eq!(
        log.lines.len(),
        LOG_FAILED.trim_end().split('\n').count() + 1,
        "each line, and one heading"
    );
    let long = read_log(&"x".repeat(5000), false);
    assert_eq!(long.lines[0].text.chars().count(), 2000);
    assert!(long.lines[0].text.ends_with('…'));
    let wide = read_log(&"é".repeat(2500), false);
    assert_eq!(wide.lines[0].text.chars().count(), 2000, "by characters");
}

#[test]
fn githubs_marks_are_shown_as_what_they_mean_and_a_steps_change_as_a_heading() {
    let log = read_log(
        &[
            "build\tInstall\t2026-10-31T17:02:37.1Z ##[group]Run pnpm install",
            "build\tInstall\t2026-10-31T17:02:37.2Z ##[endgroup]",
            "build\tInstall\t2026-10-31T17:02:37.3Z ##[warning]Old lock file",
            "build\tTests\t2026-10-31T17:02:37.4Z ##[error]It broke",
        ]
        .join("\n"),
        false,
    );
    let lines: Vec<(&str, LineKind)> = log
        .lines
        .iter()
        .map(|l| (l.text.as_str(), l.kind))
        .collect();
    assert_eq!(
        lines,
        [
            ("build › Install", LineKind::Head),
            ("Run pnpm install", LineKind::Normal),
            ("Old lock file", LineKind::Warning),
            ("build › Tests", LineKind::Head),
            ("It broke", LineKind::Error),
        ]
    );
    assert_eq!(log.errors, [4]);
}

#[test]
fn a_log_without_ghs_tab_columns_still_reads() {
    let log = read_log("line one\r\n\r\nline two\n", false);
    let texts: Vec<_> = log.lines.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(texts, ["line one", "line two"]);
    assert!(read_log("", false).lines.is_empty());
    assert!(read_log("", false).errors.is_empty());
}

#[test]
fn a_log_keeps_what_a_stamp_is_not_and_reads_one_after_a_byte_order_mark() {
    let log = read_log(
        "\u{feff}job\tstep\t\u{feff}2026-10-31T17:02:37.1099560Z hello\njob\tstep\t2026-10-31 hello\njob\tstep\t2026-10-31T17:02:37 no zone\njob\tstep\t2026-10-31T17:02:37.5Z\njob\tstep\twith\ttabs\tin it",
        false,
    );
    let texts: Vec<_> = log.lines.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "job › step",
            "hello",
            "2026-10-31 hello",
            "2026-10-31T17:02:37 no zone",
            "",
            "with\ttabs\tin it"
        ]
    );
}

#[test]
fn the_recent_runs_for_the_picker() {
    let rows = run_list(RUN_LIST).expect("a list");
    assert_eq!(rows.len(), 15);
    assert_eq!(rows[0].id, "41231065926");
    assert_eq!(rows[0].kind, Kind::Success);
    assert_eq!(rows[0].branch, "main");
    assert_eq!(rows[0].created_at, Some(at("2026-10-31T18:46:53Z")));
    assert_eq!(
        rows.iter()
            .find(|row| row.id == "41222364482")
            .map(|row| row.kind),
        Some(Kind::Failure)
    );
    assert!(rows.iter().all(|row| is_run_id(&row.id)));
    assert_eq!(run_list("nope"), None);
    assert_eq!(run_list("{}"), None);
    assert_eq!(
        run_list("[{\"displayTitle\":\"no id\"},{\"databaseId\":\"7\"},{\"databaseId\":8.0}]")
            .map(|rows| rows.len()),
        Some(1)
    );
}

#[test]
fn only_digits_make_a_run_id() {
    for good in ["1", "41231065926"] {
        assert!(is_run_id(good));
    }
    for bad in ["", "-1", "12a", " 1", "1\n2", "1\n", "１２"] {
        assert!(!is_run_id(bad), "{bad:?}");
    }
}

#[test]
fn a_run_is_read_from_what_gh_printed_and_from_nothing_else() {
    assert_eq!(Run::parse("not json"), None);
    assert_eq!(Run::parse("[]"), None);
    assert_eq!(Run::parse("null"), None);
    assert_eq!(Run::parse("\"run\""), None);
    let odd = Run::parse(
        r#"{"status":7,"jobs":[null,{"name":4,"steps":"none"},{"steps":[null,{"name":"s"}]}]}"#,
    )
    .expect("an object is a run");
    assert_eq!(odd.status, "");
    assert_eq!(odd.jobs.len(), 2, "what is not a job is left out");
    assert_eq!(odd.jobs[1].steps.len(), 1);
}
