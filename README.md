# CI status

A Hangar kit: a **CI** tab that follows a GitHub Actions run, and opens by itself
after an agent runs `git push`. It is also the example to copy when you make a
kit of your own: every part of a kit is in it, in the smallest form that works.

## What it does

- **The CI tab.** Given a run's number it shows the run's title and state on one
  line, the workflow's name as the tab's title, a badge (running, passed, failed),
  the jobs as rows with their state, their time and the step running now, and the
  run's link to copy. It polls `gh` every 5 seconds while the run is going, stops
  when it ends, and raises one desktop notification. When the run failed, the jobs
  that failed come first, with the log of the failed steps right under them, opened
  on its first error with the errors marked. Without a number it lists the 15 most
  recent runs to choose from, with their branch and age. One tab per run.
- **A hook** (Claude Code `PostToolUse` on `Bash`). When an agent in a pane runs
  `git push`, it looks for the run that push started — a run takes a few seconds
  to appear, so it retries for about half a minute — and opens the tab for it. The
  search runs detached: the agent is never kept waiting, the project's folder stays
  free to delete, and the hook always exits 0, silently, whatever is missing (Node,
  `gh`, a run, Hangar).
- **A skill** telling agents to open the tab with `kit_tab_open` when the hook did
  not, and to read a run's outcome with `gh` rather than sleeping.

If `gh` is missing or not signed in, or the tab may not run it, the tab says so and
how to fix it. It follows Hangar's theme, language and interface size, live, and holds
at the side panel's narrowest.

## Two tabs, one behaviour

The tab is written twice, and the manifest names both:

- **Native, for Hangar 2.** Rust built to WebAssembly (`native/`, committed as
  `tabs/ci.wasm`), which Hangar runs and draws itself, with its own widgets, in the
  person's theme: no browser engine.
- **The page, for Hangar 1.x** (`tabs/ci/`), drawn with
  [Hangar UI](https://github.com/hangar-ai/hangar/blob/main/docs/kit-ui.md). Hangar 1.x
  does not know the manifest's `native` field and opens it; Hangar 2 opens it too when
  the module cannot be loaded, and says so in the tab's bar.

They make the same host calls under the same permissions (`command.run` of the three
commands, `tab.setTitle`, `tab.setBadge`, `notify`) and show the same states. What the
native tab does differently, because a widget is not a web page:

- The log of a failed run is drawn in lines of one height, which is what makes a
  megabyte of them cheap: a long line is cut at the log's edge, and the log scrolls
  sideways, where the page wrapped it. Hangar is given the lines near the view and asks
  for others as the log scrolls; the bar over the log (« Error 2 of 5 », Previous,
  Next) is the tab's own.
- The log is 360 points tall rather than 56 % of the window (a native page has no `vh`).
- Text cannot be selected with the pointer: the run's link, and the command to run when
  `gh` is not signed in, have a copy button, which says « Copied ».
- The icons are named by their tooltips; there is no accessibility tree to carry `aria-*`
  yet.
- A command's own limit (30 s, 60 s for the log) is the only time-out: the page also gave
  up on a call after 30 s (130 s for a command) by itself.
- It reads its params from the start Hangar gives it, where the page asked for them, and
  behind another tab it stops its clock and polls on, which a native tab can tell
  (`tab.visibility`) and a page left to its browser.

## What it runs

Only these three, declared in `hangar-kit.json` and shown to you verbatim before
you enable the kit. The tab runs them with your `gh` login; only `{id}` changes,
and it must be digits.

| Name      | Command                                                                                                  |
| --------- | -------------------------------------------------------------------------------------------------------- |
| `ci-run`  | `gh run view {id} --json status,conclusion,displayTitle,headBranch,workflowName,url,jobs,startedAt,updatedAt` |
| `ci-list` | `gh run list --limit 15 --json databaseId,displayTitle,workflowName,headBranch,status,conclusion,createdAt` |
| `ci-log`  | `gh run view {id} --log-failed`                                                                          |

They run in the workspace's folder, so `gh` finds the repository from its git
remote. The hook runs `git rev-parse HEAD` and `gh run list --commit <sha>` in
the folder the agent pushed from, and talks to Hangar on `127.0.0.1` only. Nothing
else leaves the machine, and the tab loads no external resource.

## Install

You need Hangar 1.9.0 or later (the page opens there; Hangar 2 runs the native tab), [`gh`](https://cli.github.com) signed in
(`gh auth login`), and `node` on your PATH for the hook (without it, the hook does nothing).

1. In Hangar, Settings › Kits, add the GitHub link:
   `https://github.com/hangar-ai/kit-ci`
   (or link a local copy of this folder).
2. Enable it on a workspace, and read what you accept: the three commands, the
   `notify` permission, and the hook.
3. Restart the workspace's Claude Code panes so they load the plugin.

## How it is built

```
ci-status/
├── hangar-kit.json            the kit: id, hangar >=1.9.0, commands, the tab (its page, and its module) and params
├── icon.svg
├── .claude-plugin/plugin.json the Claude Code plugin (name = the kit's id)
├── hooks/
│   ├── hooks.json             PostToolUse on Bash → open-ci.js
│   ├── open-ci.js             reads the event, spots a push, spawns find-run.js detached, exits 0
│   ├── find-run.js            the detached search: git + gh, retries, then asks Hangar for the tab
│   └── lib.js                 what counts as a push, which runs belong to it, the call to Hangar
├── skills/ci-status/SKILL.md
├── native/                    the native tab: a Rust crate for hangar-kit-sdk (cdylib + rlib), Cargo.lock committed
│   ├── rust-toolchain.toml    the compiler that builds the committed module
│   ├── src/lib.rs             the tab: state, update, polling, title, badge, notification (controller.js)
│   ├── src/logic.rs           pure: gh's JSON and its logs → what to show (logic.js)
│   ├── src/strings.rs         the words, in en and fr
│   ├── src/view.rs            draws a state as widgets (view.js)
│   └── tests/                 cargo test: the page's tests, on the SDK's harness
├── scripts/build-native.sh    builds native/ to tabs/ci.wasm
├── tabs/ci.wasm               the module Hangar 2 runs, built and committed
├── tabs/ci/                   the page Hangar 1.x opens
│   ├── index.html             loads Hangar UI (/_hangar/kit.css, kit.js, kit-ui.js), then the files below
│   ├── logic.js               pure: gh's JSON and its logs → what to show; messages in en and fr
│   ├── controller.js          polling, title, badge, notification — takes window.hangar as an argument
│   ├── view.js                draws a state in the library's classes, with createElement + textContent, never markup
│   ├── app.js                 wires them in the page, and follows Hangar's language
│   └── style.css              only what the library does not have: no colour, no measure of its own
├── test/                      node --test, no dependency; fixtures/ are real gh output, and the native tests read them too
└── package.json, run-tests.js let `node --test kits/ci-status/` find the tests on any Node
```

Choices worth copying:

- **Hangar's library for the look.** The components, tokens, formats and the log are
  [Hangar UI](https://github.com/hangar-ai/hangar/blob/main/docs/kit-ui.md)'s; what the page adds is what only this tab knows,
  such as which lines of a CI log are errors (`logic.js`, handed to `hangar.ui.log`).
- **A redraw changes only what changed.** The page is redrawn every second while a run
  goes: `view.js` rebuilds a part only when what it shows changed, and writes the clocks
  in place, so nothing takes the focus from a button or scrolls a log back.
- **Logic out of the page.** `logic.js` and `controller.js` touch neither the DOM
  nor Hangar except through arguments, so the tests drive them with a fake
  `window.hangar` and a fake clock (`test/helpers.js`).
- **A native tab beside the page.** The manifest stays at schema 1 with its HTML `entry`
  and gains `"native": "tabs/ci.wasm"`: an older Hangar ignores the field, a native one
  runs the module, and the page is the fallback. `logic.rs` is `logic.js` without a
  widget or a call in it, so the same cases hold both; the error-line classifier is
  written by hand (one comparison turns away most lines, and a megabyte of log is read in
  a few milliseconds on the host, well inside the 250 ms Hangar gives an update) and
  `native/tests/golden/error-lines.json` holds it to the page's regular expression on
  1 100 lines.
- **The module is the same bytes on every machine.** The compiler is pinned, and the paths a
  module remembers are made the same by `scripts/build-native.sh`, so CI can build it again
  and fail when the committed one is stale.
- **Params, one tab per run.** The tab declares `params.id` with the pattern
  `[0-9]+`. Hangar checks a caller's value against it, and again before any
  command runs; the page re-checks, because a page treats params like form input.
- **Declared commands, not a shell.** Each is an argv; `{id}` is its only
  placeholder and carries a rule. A value starting with `-` is refused by Hangar,
  so an id can never become an option.
- **Badge and title are set on load**, as a stale « running » is worse than none,
  and taken away (`state: null`) when the tab can follow nothing.
- **A hook fails quietly.** `uncaughtException` → exit 0, a two-second timeout on
  the call, work done detached.

## Tests

```
node --test kits/ci-status/
cargo test --manifest-path native/Cargo.toml
```

The Node tests need no install step. They cover the page's logic on recorded `gh` output (running,
success, failure, not signed in, `gh` absent), which line of a real failed log is its
first error, the controller's polling and its single notification, what the view draws
in each state, on a stand-in for `hangar.ui`, the hook's recognition of `git push`
(and not `git push --help` or `echo git push`), and the manifest against the rules
in [docs/kits.md](https://github.com/hangar-ai/hangar/blob/main/docs/kits.md) — Hangar's own validator is Rust, so the Node
test restates its rules, its `native` field included. `test/fixtures/run-running.json`
is the recorded success run with its statuses turned to « in progress », the one state
that cannot be recorded after the fact. `test/native.test.js` runs the committed
`tabs/ci.wasm` through Hangar's ABI with Node alone: it imports only Hangar's log and
clock, exports what a tab must, asks for the run, and follows it.

The Rust tests are the page's, case for case, on `hangar_kit_sdk::testing::Harness`: a
fake Hangar answers the tab's calls as Hangar does, with a clock the test moves by hand
(`native/tests/support`). `logic.rs` is `logic.test.js`, `controller.rs` is
`controller.test.js` and `view.rs` is `view.test.js`, which looks at the widgets Hangar
draws where the page's test looked at classes. They add what the page's script did not do:
the log's windows and the bar between errors, a tab asked for again, a tab behind another,
a megabyte of log.

## This repository

`node --test` runs the tests, with Node alone. Hangar's own files are not in this repository. CI takes Hangar UI and the docs from the Hangar tag the
manifest's `hangar` names, and `node test/hangar-ui.js --get` does the same here. Without them, these tests
skip what needs those files, and say so:

- the classes the tab names are the library's, when the library is around
- the permissions used are in the docs, when the docs are around

CI calls [kit-check](https://github.com/hangar-ai/.github/blob/main/.github/workflows/kit-check.yml),
and checks the native tab: `cargo fmt`, `cargo clippy`, the Rust tests, and a rebuild of
`tabs/ci.wasm` that must leave the committed file as it is. A release is
a tag, `v` and the manifest's version, and it reaches Hangar's gallery once
[the catalog](https://github.com/hangar-ai/catalog) is updated and reviewed.

### Building the module

Hangar runs the `.wasm` a kit ships and never builds one, so the module is committed, in
the commit that changes its source:

```
scripts/build-native.sh        # native/ → tabs/ci.wasm; rustup fetches native/rust-toolchain.toml's compiler
```

The SDK comes from `hangar-ai/hangar`'s `feat/native-rust` branch until a release tag holds
it; `native/Cargo.lock` pins the commit. To try the tab, link this folder in a Hangar 2
(Settings › Kits), open the tab, and reload it from its bar after each build; or, from a
clone of Hangar, `cargo run -p hangar-kit-host --example preview -- path/to/kit-ci/tabs/ci.wasm`
(`--light`, `--fr`), a pretend Hangar that grants no command, which shows the tab's denied state.
