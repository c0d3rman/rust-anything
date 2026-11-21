"use strict";

// node gen-core-inputs.cjs <out.jsonl>
// Linter-core cases: directive comments, inline config, unused directives, fixes, formats,
// severities, parse errors and CLI options around them.

const fs = require("node:fs");

const out = process.argv[2];
const lines = [];
const base = rules => ["--no-config-lookup", "--rule", JSON.stringify(rules)];
const R = { eqeqeq: "error", "no-debugger": "warn", "no-var": "error", curly: "error", "no-empty": "error" };

const sources = {
	plain: "var a = 1;\nif (a == 2) debugger;\n",
	disableAll: "/* eslint-disable */\nvar a = 1;\nif (a == 2) debugger;\n",
	disableRule: "/* eslint-disable eqeqeq */\nvar a = 1;\nif (a == 2) debugger;\n/* eslint-enable eqeqeq */\nif (a == 3) {}\n",
	disableLine: "var a = 1; // eslint-disable-line no-var\nif (a == 2) debugger; // eslint-disable-line\n",
	disableNext: "// eslint-disable-next-line no-var, eqeqeq\nvar a = 1;\n/* eslint-disable-next-line */\nif (a == 2) debugger;\n",
	disableNextBlockMulti: "/* eslint-disable-next-line\n   no-var,\n   eqeqeq -- reasons\n*/\nvar a = 1 == 2;\n",
	justification: "// eslint-disable-next-line no-var -- legacy code\nvar a = 1;\nvar b = 2; // eslint-disable-line no-var -- also legacy\n",
	unusedDisable: "// eslint-disable-next-line eqeqeq\nvar a = 1;\n/* eslint-disable no-debugger */\nconst b = 2;\n/* eslint-enable no-debugger */\n",
	unusedAll: "/* eslint-disable */\nconst a = 1;\n",
	unusedLine: "const a = 1; // eslint-disable-line\nconst b = 2; // eslint-disable-line no-var, eqeqeq\n",
	unusedEnable: "/* eslint-enable */\n/* eslint-enable eqeqeq */\nconst a = 1;\n",
	duplicateDisable: "/* eslint-disable eqeqeq */\n/* eslint-disable eqeqeq */\nif (a == b) {}\n",
	partialUnused: "// eslint-disable-next-line eqeqeq, no-var\nvar a = 1;\n",
	inlineConfig: "/* eslint eqeqeq: \"off\", curly: [\"error\", \"multi\"] */\nif (a == b) { c(); }\n",
	inlineConfigSeverity: "/* eslint no-debugger: 2 */\ndebugger;\n/* eslint eqeqeq: 1 */\nif (a == b) {}\n",
	inlineConfigSeverityOnly: "/* eslint eqeqeq: \"warn\" */\nif (a == b) {}\n",
	inlineConfigBad: "/* eslint eqeqeq: [\"error\", \"sometimes\"] */\nif (a == b) {}\n",
	inlineConfigUnknownRule: "/* eslint no-such-rule: \"error\" */\nconst a = 1;\n",
	inlineConfigSyntax: "/* eslint eqeqeq: [error */\nif (a == b) {}\n",
	inlineConfigLineComment: "// eslint eqeqeq: off\nif (a == b) {}\n",
	inlineConfigTwice: "/* eslint eqeqeq: \"off\" */\n/* eslint eqeqeq: \"error\" */\nif (a == b) {}\n",
	inlineConfigUnused: "/* eslint eqeqeq: \"error\" */\nif (a === b) {}\n",
	globalComment: "/* global foo, bar: writable, baz: readonly */\nfoo = 1;\n",
	globalsComment: "/* globals foo */\nfoo();\n",
	exportedComment: "/* exported foo */\nvar foo = 1;\n",
	eslintEnv: "/* eslint-env node */\nrequire('x');\n",
	disableUnknown: "// eslint-disable-next-line no-such-rule\nif (a == b) {}\n",
	disableBlockOnLine: "/* eslint-disable-line */ if (a == b) {}\n",
	disableLineMultiLineComment: "if (a == b) {} /* eslint-disable-line\n*/\n",
	shebang: "#!/usr/bin/env node\nvar a = 1;\n",
	bom: "﻿var a = 1;\nif (a == 2) {}\n",
	crlf: "var a = 1;\r\nif (a == 2) {\r\n}\r\n",
	cr: "var a = 1;\rif (a == 2) {\r}\r",
	unicode: "var é = '日本語'; if (é == '😀') debugger;\n",
	emojiColumns: "const s = '😀😀'; if (s == 1) {}\n",
	tabs: "\tif (a == b) {\n\t\tdebugger;\n\t}\n",
	parseError: "var a = ;\n",
	parseErrorUnterminated: "const s = 'abc\n",
	parseErrorJsx: "const x = <div></span>;\n",
	parseErrorModule: "return 1;\n",
	importInScript: "import a from 'a';\n",
	empty: "",
	onlyComments: "// hello\n/* world */\n",
	noTrailingNewline: "if (a == b) debugger",
	manyProblems: Array.from({ length: 30 }, (_, i) => `if (x${i} == ${i}) {}`).join("\n"),
	sameLocation: "if (a == b) var c = d == e;\n",
	nestedFixes: "if (a) if (b) c();\nelse d();\n",
	overlappingFixes: "var a = 1, b = 2; var c = a == b;\n",
	fixCascade: "var a = 1;\nvar b = a;\nvar c = b;\n",
	regex: "var r = /a == b/g; if (r == 1) {}\n",
	template: "var t = `${a == b}`;\n",
	classFields: "class A { #x = 1; static { if (this.#x == 1) {} } }\n",
	topLevelAwait: "await foo(); if (a == b) {}\n",
	hashbangDirective: "#!/usr/bin/env node\n// eslint-disable-next-line eqeqeq\nif (a == b) {}\n",
};

function add(name, args, stdin) {
	lines.push(JSON.stringify({ name: `core-${name}`, args, stdin }));
}

for (const [name, src] of Object.entries(sources)) {
	add(`${name}-json`, [...base(R), "-f", "json", "--stdin", "--stdin-filename", "input.js"], src);
	add(`${name}-stylish`, [...base(R), "--stdin", "--stdin-filename", "input.js"], src);
	add(`${name}-fix`, [...base(R), "-f", "json", "--fix-dry-run", "--stdin", "--stdin-filename", "input.js"], src);
	add(`${name}-cjs`, [...base(R), "-f", "json", "--stdin", "--stdin-filename", "input.cjs"], src);
}

const opts = {
	"unused-error": ["--report-unused-disable-directives"],
	"unused-sev-off": ["--report-unused-disable-directives-severity", "off"],
	"unused-sev-warn": ["--report-unused-disable-directives-severity", "1"],
	"unused-sev-error": ["--report-unused-disable-directives-severity", "error"],
	"unused-inline-warn": ["--report-unused-inline-configs", "warn"],
	"unused-inline-error": ["--report-unused-inline-configs", "error"],
	"no-inline": ["--no-inline-config"],
	"no-inline-unused": ["--no-inline-config", "--report-unused-disable-directives"],
	quiet: ["--quiet"],
	"max-warnings-0": ["--max-warnings", "0"],
	"max-warnings-5": ["--max-warnings", "5"],
	"fix-type-layout": ["--fix-dry-run", "--fix-type", "layout"],
	"fix-type-problem": ["--fix-dry-run", "--fix-type", "problem,suggestion"],
	"fix-type-directive": ["--fix-dry-run", "--fix-type", "directive", "--report-unused-disable-directives"],
	"fix-unused": ["--fix-dry-run", "--report-unused-disable-directives"],
	"global-flag": ["--global", "foo,bar:true"],
	"jsx": ["--parser-options", "ecmaFeatures:{jsx:true}"],
	"global-return": ["--parser-options", "ecmaFeatures:{globalReturn:true}"],
	"implied-strict": ["--parser-options", "ecmaFeatures:{impliedStrict:true}"],
	"no-color": ["--no-color"],
	"color": ["--color"],
	"exit-on-fatal": ["--exit-on-fatal-error"],
};
for (const [name, extra] of Object.entries(opts)) {
	for (const src of ["plain", "unusedDisable", "inlineConfigUnused", "parseError", "disableLine", "partialUnused", "unusedLine"]) {
		add(`opt-${name}-${src}-json`, [...base(R), ...extra, "-f", "json", "--stdin", "--stdin-filename", "input.js"], sources[src]);
		add(`opt-${name}-${src}-stylish`, [...base(R), ...extra, "--stdin", "--stdin-filename", "input.js"], sources[src]);
	}
}

const ruleVariants = {
	"no-rules": {},
	"warn-only": { eqeqeq: "warn" },
	"numeric": { eqeqeq: 2, "no-debugger": 1, "no-var": 0 },
	"options-array": { eqeqeq: ["error", "smart"], curly: ["warn", "multi-line"] },
	"off": { eqeqeq: "off" },
	"unknown-rule": { "no-such-rule": "error" },
	"bad-severity": { eqeqeq: "fatal" },
	"bad-options": { eqeqeq: ["error", "sometimes"] },
	"order-1": { "no-debugger": "error", eqeqeq: "error", curly: "error" },
	"order-2": { curly: "error", eqeqeq: "error", "no-debugger": "error" },
};
for (const [name, rules] of Object.entries(ruleVariants)) {
	add(`rules-${name}-json`, [...base(rules), "-f", "json", "--stdin", "--stdin-filename", "input.js"], sources.sameLocation + sources.plain);
	add(`rules-${name}-stylish`, [...base(rules), "--stdin", "--stdin-filename", "input.js"], sources.sameLocation + sources.plain);
}
add("two-rule-flags", ["--no-config-lookup", "--rule", "eqeqeq: error", "--rule", "no-debugger: warn", "--stdin", "--stdin-filename", "input.js"], sources.plain);
add("levn-rule", ["--no-config-lookup", "--rule", "eqeqeq: [error, smart]", "-f", "json", "--stdin", "--stdin-filename", "input.js"], sources.plain);
add("no-filename", ["--no-config-lookup", "--rule", "eqeqeq: error", "-f", "json", "--stdin"], sources.plain);
add("mjs", [...base(R), "-f", "json", "--stdin", "--stdin-filename", "input.mjs"], sources.plain);
add("subdir", [...base(R), "-f", "json", "--stdin", "--stdin-filename", "src/lib/input.js"], sources.plain);
add("ts-file", [...base(R), "-f", "json", "--stdin", "--stdin-filename", "input.ts"], sources.plain);
add("json-with-metadata", [...base(R), "-f", "json-with-metadata", "--stdin", "--stdin-filename", "input.js"], sources.plain);
add("format-html", [...base(R), "-f", "html", "--stdin", "--stdin-filename", "input.js"], sources.plain);
add("config-lookup", ["--rule", "eqeqeq: error", "--stdin", "--stdin-filename", "input.js"], sources.plain);
add("version", ["--version"], "");
add("help", ["--help"], "");

fs.writeFileSync(out, `${lines.join("\n")}\n`);
console.error(`${lines.length} cases`);
