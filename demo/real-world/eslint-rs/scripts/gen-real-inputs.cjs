"use strict";

// node gen-real-inputs.cjs <rules.txt> <out.jsonl> <label>=<dir> [<label>=<dir> ...]

const fs = require("node:fs");
const path = require("node:path");

const [rulesFile, out, ...roots] = process.argv.slice(2);
const rules = fs
	.readFileSync(rulesFile, "utf8")
	.split("\n")
	.map(s => s.trim())
	.filter(s => s && !s.startsWith("#"));

const NODE_GLOBALS =
	"require,module,exports,__dirname,__filename,process,console,Buffer,setTimeout,clearTimeout,setInterval,clearInterval,setImmediate,queueMicrotask,structuredClone,URL,TextEncoder,TextDecoder,AbortController,globalThis,window,document,self,navigator";

function walk(dir, acc) {
	for (const entry of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : 1))) {
		if (entry.name === "node_modules" || entry.name.startsWith(".")) {
			continue;
		}
		const p = path.join(dir, entry.name);
		if (entry.isDirectory()) {
			walk(p, acc);
		} else if (/\.(?:js|mjs|cjs)$/u.test(entry.name)) {
			acc.push(p);
		}
	}
	return acc;
}

const ruleConf = JSON.stringify(Object.fromEntries(rules.map(r => [r, "error"])));
const lines = [];
for (const root of roots) {
	const [label, dir] = root.split("=");
	for (const file of walk(dir, [])) {
		const rel = path.relative(dir, file);
		const text = fs.readFileSync(file, "utf8");
		const ext = path.extname(file);
		const slug = `${label}-${rel.replace(/[^A-Za-z0-9]+/gu, "-")}`;
		const base = ["--no-config-lookup", "--rule", ruleConf, "--global", NODE_GLOBALS];
		lines.push(JSON.stringify({ name: `real-${slug}`, args: [...base, "-f", "json", "--fix-dry-run", "--stdin", "--stdin-filename", `input${ext}`], stdin: text }));
		lines.push(JSON.stringify({ name: `real-stylish-${slug}`, args: [...base, "--stdin", "--stdin-filename", `input${ext}`], stdin: text }));
	}
}
fs.writeFileSync(out, `${lines.join("\n")}\n`);
console.error(`${lines.length} cases`);
