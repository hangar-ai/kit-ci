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

## A native tab

The tab is native (Hangar 2.0 or later): Rust built to WebAssembly (`native/`, committed as
`tabs/ci.wasm`, which the manifest names as the tab's `entry`), which Hangar runs and draws
itself, with its own widgets, in the person's theme: no browser engine. It makes only these host
calls, under the permissions the manifest declares (`command.run` of the three commands,
`tab.setTitle`, `tab.setBadge`, `notify`). Because a widget is not a web page:

- The log of a failed run is drawn in lines of one height, which is what makes a
  megabyte of them cheap: a long line is cut at the log's edge, and the log scrolls
  sideways. Hangar is given the lines near the view and asks for others as the log
  scrolls; the bar over the log (« Error 2 of 5 », Previous, Next) is the tab's own.
- The log is 360 points tall rather than a share of the window (a native page has no `vh`).
- Text cannot be selected with the pointer: the run's link, and the command to run when
  `gh` is not signed in, have a copy button, which says « Copied ».
- The icons are named by their tooltips; there is no accessibility tree to carry `aria-*`
  yet.
- A command's own limit (30 s, 60 s for the log) is the only time-out.
- It reads its params from the start Hangar gives it, and behind another tab it stops its
  clock and polls on (`tab.visibility`).

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

You need Hangar 2.0.0 or later, [`gh`](https://cli.github.com) signed in
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
├── hangar-kit.json            the kit: schema 2, id, hangar >=2.0.0, commands, the tab (its `entry` is the module) and params
├── icon.svg
├── .claude-plugin/plugin.json the Claude Code plugin (name = the kit's id)
├── hooks/
│   ├── hooks.json             PostToolUse on Bash → open-ci.js
│   ├── open-ci.js             reads the event, spots a push, spawns find-run.js detached, exits 0
│   ├── find-run.js            the detached search: git + gh, retries, then asks Hangar for the tab
│   └── lib.js                 what counts as a push, which runs belong to it, the call to Hangar
├── skills/ci-status/SKILL.md
├── native/                    the tab: a Rust crate for hangar-kit-sdk (cdylib + rlib), Cargo.lock committed
│   ├── rust-toolchain.toml    the compiler that builds the committed module
│   ├── src/lib.rs             the tab: state, update, polling, title, badge, notification
│   ├── src/logic.rs           pure: gh's JSON and its logs → what to show
│   ├── src/strings.rs         the words, in en and fr
│   ├── src/view.rs            draws a state as widgets
│   └── tests/                 cargo test, on the SDK's harness
├── scripts/build-native.sh    builds native/ to tabs/ci.wasm (--check: fails if the committed one is stale)
├── tabs/ci.wasm               the module Hangar runs, built and committed
├── test/                      node --test, no dependency: the manifest, the hook, the module; fixtures/ are real gh output, and the native tests read them too
└── package.json, run-tests.js let `node --test kits/ci-status/` find the tests on any Node
```

Choices worth copying:

- **Hangar's widgets for the look.** The tab draws with the SDK's widgets, in the person's theme;
  what it adds is what only this tab knows, such as which lines of a CI log are errors
  (`logic.rs`, handed to the log widget).
- **Logic apart from the widgets.** `logic.rs` has no widget and no call in it, so its tests
  run on recorded `gh` output, and the tab's on a pretend Hangar and a clock the test moves.
- **The classifier of error lines is written by hand.** One comparison turns away most lines,
  and a megabyte of log is read in a few milliseconds on the host, well inside the 250 ms Hangar
  gives an update; `native/tests/golden/error-lines.json` holds it to the answers of the regular
  expression the kit's earlier HTML tab used, on 1 100 lines.
- **The module is the same bytes on every machine.** The compiler is pinned, and the paths a
  module remembers are made the same by `scripts/build-native.sh`, so CI can build it again
  and fail when the committed one is stale.
- **Params, one tab per run.** The tab declares `params.id` with the pattern
  `[0-9]+`. Hangar checks a caller's value against it, and again before any
  command runs; the tab re-checks, because params are input.
- **Declared commands, not a shell.** Each is an argv; `{id}` is its only
  placeholder and carries a rule. A value starting with `-` is refused by Hangar,
  so an id can never become an option.
- **Badge and title are set on load**, as a stale « running » is worse than none,
  and taken away (`state: null`) when the tab can follow nothing.
- **A hook fails quietly.** `uncaughtException` → exit 0, a two-second timeout on
  the call, work done detached.

## Tests

```
cargo test --manifest-path native/Cargo.toml
node --test kits/ci-status/
scripts/build-native.sh --check
```

The Rust tests run on `hangar_kit_sdk::testing::Harness`: a fake Hangar answers the tab's calls
as Hangar does, with a clock the test moves by hand (`native/tests/support`). They cover the
logic on recorded `gh` output (running, success, failure, not signed in, `gh` absent), which
line of a real failed log is its first error, the polling and the single notification, what the
view draws in each state (the widgets Hangar draws), the log's windows and the bar between
errors, a tab asked for again, a tab behind another, and a megabyte of log.
`test/fixtures/run-running.json` is the recorded success run with its statuses turned to
« in progress », the one state that cannot be recorded after the fact.

The Node tests need no install step. They cover the hook's recognition of `git push` (and not
`git push --help` or `echo git push`), the plugin, and the manifest against the rules in
Hangar's `docs/native/kits-native.md`: Hangar's own validator is Rust, so the Node test restates
its rules. `test/native.test.js` runs the committed `tabs/ci.wasm` through Hangar's ABI with Node
alone: it imports only Hangar's log and clock, exports what a tab must, asks for the run, and
follows it.

## This repository

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
