"use strict";
// Shared by the tests: the kit's folder, and the recorded `gh` output that the module is run on.
const fs = require("node:fs");
const path = require("node:path");

const KIT = path.join(__dirname, "..");
const fixture = (name) => fs.readFileSync(path.join(__dirname, "fixtures", name), "utf8");

module.exports = { KIT, fixture };
