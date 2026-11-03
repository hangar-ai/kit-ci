"use strict";
// The committed module, tabs/ci.wasm, run the way Hangar runs it: the ABI of
// docs/native/kits-native.md, JSON in and out of the module's memory, with only
// Hangar's log and clock to import. The tab's behaviour is tested in Rust, on the
// SDK's harness (native/tests); this holds the built file to be a module Hangar
// loads, and one that starts. Without it a stale or broken build would only show in Hangar.
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { KIT, fixture } = require("./helpers.js");

const manifest = JSON.parse(fs.readFileSync(path.join(KIT, "hangar-kit.json"), "utf8"));
const tab = manifest.tabs[0];
const bytes = fs.readFileSync(path.join(KIT, tab.entry));
const compiled = new WebAssembly.Module(bytes);

const ID = "41222364482";
const START = {
  abi: 1,
  hangar: "2.0.0",
  kit: { id: manifest.id, name: manifest.name, version: manifest.version },
  tab: { id: tab.id, params: { id: ID } },
  theme: { scheme: "dark", locale: "en" },
  permissions: tab.permissions,
  visible: true,
  utcOffset: 0,
};

/** The module, started: `init`, `update` and `view` take and answer JSON. */
function instantiate() {
  let memory;
  const logged = [];
  const instance = new WebAssembly.Instance(compiled, {
    hangar: {
      log: (level, ptr, len) => logged.push([level, Buffer.from(memory.buffer, ptr, len).toString("utf8")]),
      now: () => BigInt(Date.parse("2026-10-31T18:48:37Z")),
    },
  });
  memory = instance.exports.memory;
  const read = (packed) => {
    const ptr = Number(packed >> 32n);
    const len = Number(packed & 0xffffffffn);
    return JSON.parse(Buffer.from(memory.buffer, ptr, len).toString("utf8"));
  };
  const send = (name, message) => {
    const text = Buffer.from(JSON.stringify(message), "utf8");
    const ptr = instance.exports.hangar_alloc(text.length);
    Buffer.from(memory.buffer, ptr, text.length).set(text);
    return read(instance.exports[name](ptr, text.length));
  };
  return {
    logged,
    abi: () => instance.exports.hangar_abi_version(),
    init: (start) => send("hangar_init", start),
    update: (input) => send("hangar_update", input),
    view: () => read(instance.exports.hangar_view()),
  };
}

/** Every widget of a view, parents before children, whatever it holds them in. */
function widgets(node, found = []) {
  if (Array.isArray(node)) node.forEach((child) => widgets(child, found));
  else if (node && typeof node === "object") {
    if (typeof node.type === "string") found.push(node);
    Object.values(node).forEach((child) => widgets(child, found));
  }
  return found;
}
const headline = (view) => widgets(view.root).find((node) => node.type === "header").title;
const answer = (id, stdout) => ({
  type: "reply",
  id,
  result: { exitCode: 0, stdout: fixture(stdout), stderr: "", timedOut: false, truncated: false },
});

test("the module imports Hangar's log and clock and nothing else, and exports the tab ABI", () => {
  assert.deepEqual([...bytes.subarray(0, 4)], [0x00, 0x61, 0x73, 0x6d], "WebAssembly");
  const imports = WebAssembly.Module.imports(compiled).map(({ module, name, kind }) => `${module}.${name}:${kind}`);
  assert.deepEqual(imports.sort(), ["hangar.log:function", "hangar.now:function"], "no WASI, no glue");
  const exports = WebAssembly.Module.exports(compiled).map(({ name }) => name);
  assert.deepEqual(exports.sort(), ["hangar_abi_version", "hangar_alloc", "hangar_init", "hangar_update", "hangar_view", "memory"]);
});

test("it starts: the ABI it speaks, the run it asks for, the page it draws while waiting", () => {
  const module = instantiate();
  assert.equal(module.abi(), 1);
  const output = module.init(START);
  assert.deepEqual(output.calls.map((call) => [call.method, call.params]), [["command.run", { name: "ci-run", args: { id: ID } }]]);
  assert.ok(output.subscriptions.events.includes("tab.params"), "it hears of a new run");
  assert.equal(headline(module.view()), `Run #${ID}`);
  assert.ok(widgets(module.view().root).some((node) => node.type === "empty" && node.kind === "loading"));
});

test("it follows a run: title and badge from the answer, then the poll", () => {
  const module = instantiate();
  const [call] = module.init(START).calls;
  const output = module.update(answer(call.id, "run-running.json"));
  assert.deepEqual(output.calls.map((c) => [c.method, c.params]), [
    ["tab.setTitle", { title: "CI" }],
    ["tab.setBadge", { state: "running", text: "running" }],
  ]);
  assert.ok(output.subscriptions.timers.some((timer) => timer.id.startsWith("~after:poll:") && timer.everyMs === 5000));
  assert.ok(output.subscriptions.timers.some((timer) => timer.id === "tick" && timer.everyMs === 1000));
  assert.equal(headline(module.view()), "Point the site's downloads to 1.8.0");
});

test("it opens a failed run on the first error of its log", () => {
  const module = instantiate();
  const [run] = module.init(START).calls;
  const [, , log] = module.update(answer(run.id, "run-failure.json")).calls;
  assert.deepEqual([log.method, log.params], ["command.run", { name: "ci-log", args: { id: ID } }]);
  const output = module.update(answer(log.id, "log-failed.txt"));
  assert.equal(output.operations.length, 1);
  assert.equal(output.operations[0].type, "scrollTo");
  const lines = widgets(module.view().root).find((node) => node.type === "log");
  assert.equal(lines.total, 151);
  assert.equal(lines.id, output.operations[0].id);
  assert.match(lines.lines.find((line) => line.kind === "error").text, /panicked at src\/accounts\.rs:688:9/);
  assert.deepEqual(module.logged.filter(([level]) => level >= 3), [], "nothing logged as an error or a panic");
});
