"use strict";

// node harvest-rule-tests.cjs <eslint clone> <rules.txt> <out.jsonl> [globals module dir]

const Module = require("node:module");
const path = require("node:path");
const fs = require("node:fs");

const [clone, rulesFile, out, globalsDir] = process.argv.slice(2);
const rules = fs
	.readFileSync(rulesFile, "utf8")
	.split("\n")
	.map(s => s.trim())
	.filter(s => s && !s.startsWith("#"));

const collected = [];
let current = null;

class FakeRuleTester {
	constructor(config) {
		this.config = config || {};
	}
	static only(item) {
		return typeof item === "string" ? { code: item } : item;
	}
	static setDefaultConfig() {}
	static resetDefaultConfig() {}
	static getDefaultConfig() {
		return {};
	}
	run(name, _rule, tests) {
		for (const kind of ["valid", "invalid"]) {
			for (const item of tests[kind] || []) {
				const t = typeof item === "string" ? { code: item } : item;
				collected.push({
					rule: current,
					runName: name,
					kind,
					tester: this.config,
					test: t,
				});
			}
		}
	}
}

const PARSER = Symbol("custom parser");
const origLoad = Module._load;
Module._load = function (request, parent, isMain) {
	if (/rule-tester\/rule-tester$|lib\/rule-tester$/u.test(request)) {
		return FakeRuleTester;
	}
	if (request === "globals" && globalsDir) {
		return origLoad.call(
			this,
			path.join(globalsDir, "node_modules/globals"),
			parent,
			isMain,
		);
	}
	if (
		request.startsWith("custom-parser:") ||
		request.includes("fixtures/parsers") ||
		request.includes("fixture-parser") ||
		request.startsWith("@typescript-eslint") ||
		request.startsWith("@babel")
	) {
		if (request.startsWith("custom-parser:")) {
			return { [PARSER]: request, parse() {}, parseForESLint() {} };
		}
		const stub = name => `custom-parser:${name}`;
		stub[PARSER] = request;
		stub.parse = () => {};
		stub.parseForESLint = () => {};
		return stub;
	}
	return origLoad.call(this, request, parent, isMain);
};
globalThis.describe = (_n, fn) => fn && fn();
globalThis.it = () => {};
globalThis.it.only = () => {};

const missing = [];
for (const rule of rules) {
	current = rule;
	const file = path.join(clone, "tests/lib/rules", `${rule}.js`);
	if (!fs.existsSync(file)) {
		missing.push(rule);
		continue;
	}
	require(file);
}

function isParser(p) {
	if (!p) {
		return false;
	}
	if ((typeof p === "object" || typeof p === "function") && p[PARSER]) {
		return true;
	}
	if (typeof p === "object" && (p.parse || p.parseForESLint)) {
		return true;
	}
	return false;
}

function merge(a, b) {
	const out = { ...(a || {}) };
	for (const [k, v] of Object.entries(b || {})) {
		if (v && typeof v === "object" && !Array.isArray(v) && out[k] && typeof out[k] === "object") {
			out[k] = merge(out[k], v);
		} else {
			out[k] = v;
		}
	}
	return out;
}

const lines = [];
const skipped = {};
const counters = {};
for (const c of collected) {
	const t = c.test;
	const lo = merge(c.tester.languageOptions, t.languageOptions);
	const why = [];
	if (isParser(lo.parser)) {
		why.push("parser");
	}
	if (t.settings || c.tester.settings || t.plugins) {
		why.push("settings or plugins");
	}
	const testPlugins = Object.keys(c.tester.plugins || {});
	if (typeof t.code === "string" && testPlugins.some(name => t.code.includes(`${name}/`))) {
		why.push("uses a test plugin's rule");
	}
	if (typeof t.code !== "string") {
		why.push("no code");
	}
	if (typeof t.code === "string" && /\p{Surrogate}/u.test(t.code)) {
		why.push("lone surrogate (cannot pass through stdin as UTF-8)");
	}
	if (lo.processor || t.processor) {
		why.push("processor");
	}
	if (why.length) {
		skipped[why[0]] = (skipped[why[0]] || 0) + 1;
		continue;
	}
	const n = (counters[c.rule] = (counters[c.rule] || 0) + 1);
	const severity = "error";
	const ruleConf = { [c.runName]: [severity, ...(t.options || [])] };
	const args = ["--no-config-lookup", "--rule", JSON.stringify(ruleConf)];
	const globals = Object.entries(lo.globals || {}).map(([k, v]) =>
		v === true || v === "writable" || v === "writeable" ? `${k}:true` : k,
	);
	if (globals.length) {
		args.push("--global", globals.join(","));
	}
	const po = { ...(lo.parserOptions || {}) };
	delete po.ecmaVersion;
	delete po.sourceType;
	if (Object.keys(po).length) {
		args.push("--parser-options", JSON.stringify(po));
	}
	const ext =
		lo.sourceType === "script" || lo.sourceType === "commonjs" ? "cjs" : "js";
	const json = n % 5 !== 0;
	if (json) {
		args.push("-f", "json");
	}
	if (c.kind === "invalid" && t.output !== void 0 && t.output !== null) {
		args.push("--fix-dry-run");
	}
	args.push("--stdin", "--stdin-filename", `input.${ext}`);
	lines.push(
		JSON.stringify({
			name: `${c.rule}-${c.kind === "valid" ? "v" : "i"}${n}`,
			args,
			stdin: t.code,
		}),
	);
}

fs.writeFileSync(out, lines.length ? `${lines.join("\n")}\n` : "");
console.error(
	JSON.stringify({ cases: lines.length, collected: collected.length, skipped, missing }),
);
