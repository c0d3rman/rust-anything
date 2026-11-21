// node linter-node.mjs <eslint clone> <inputs.jsonl> <names.txt> [iterations]
// ESLint's Linter#verifyAndFix in process, warm, on the same files and flags as the CLI cases.
// Prints per-file microseconds (median of the iterations) as JSON lines.
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";

const [clone, inputs, namesFile, itArg] = process.argv.slice(2);
const require = createRequire(import.meta.url);
const { Linter } = require(`${clone}/lib/api.js`);
const iterations = Number(itArg || 10);
const names = new Set(readFileSync(namesFile, "utf8").split("\n").filter(Boolean));
const cases = readFileSync(inputs, "utf8").split("\n").filter(Boolean).map(l => JSON.parse(l)).filter(c => names.has(c.name.toLowerCase()) || names.has(c.name));
const linter = new Linter();
const configOf = args => {
	const rules = JSON.parse(args[args.indexOf("--rule") + 1]);
	const g = args.includes("--global") ? args[args.indexOf("--global") + 1].split(",") : [];
	const globals = Object.fromEntries(g.map(x => x.endsWith(":true") ? [x.slice(0, -5), "writable"] : [x, "readonly"]));
	const file = args[args.indexOf("--stdin-filename") + 1];
	return { file, fix: args.includes("--fix-dry-run"), config: [{ files: ["**/*.js", "**/*.mjs", "**/*.cjs"] }, { files: ["**/*.cjs"], languageOptions: { sourceType: "commonjs" } }, { rules, languageOptions: { globals }, linterOptions: { reportUnusedDisableDirectives: "warn" } }] };
};
const median = a => { const s = [...a].sort((x, y) => x - y); return s[Math.floor(s.length / 2)]; };
for (const c of cases) {
	const { file, fix, config } = configOf(c.args);
	const run = () => fix ? linter.verifyAndFix(c.stdin, config, { filename: file }) : linter.verify(c.stdin, config, { filename: file });
	for (let i = 0; i < 3; i++) run();
	const t = [];
	for (let i = 0; i < iterations; i++) {
		const s = process.hrtime.bigint();
		run();
		t.push(Number(process.hrtime.bigint() - s) / 1e3);
	}
	console.log(JSON.stringify({ name: c.name, us: median(t) }));
}
