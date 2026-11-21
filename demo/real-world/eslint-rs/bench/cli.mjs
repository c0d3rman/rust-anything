// node cli.mjs <eslint bin> <front door bin> <inputs.jsonl> <names.txt> [runs]
// One process per case, alternating ESLint's CLI and the front door, stdin piped, from an
// empty folder. Prints the median and fastest wall time per side and the total.
import { spawnSync } from "node:child_process";
import { readFileSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const [eslintArg, frontArg, inputs, namesFile, runsArg] = process.argv.slice(2);
const eslint = resolve(eslintArg);
const front = resolve(frontArg);
const runs = Number(runsArg || 3);
const names = new Set(readFileSync(namesFile, "utf8").split("\n").filter(Boolean));
const cases = readFileSync(inputs, "utf8").split("\n").filter(Boolean).map(l => JSON.parse(l)).filter(c => names.has(c.name.toLowerCase()) || names.has(c.name));
const cwd = mkdtempSync(join(tmpdir(), "eslint-bench-"));
const time = (cmd, args, input) => {
	const t = process.hrtime.bigint();
	const r = spawnSync(cmd, args, { input, cwd, encoding: "utf8", maxBuffer: 1 << 28 });
	const ms = Number(process.hrtime.bigint() - t) / 1e6;
	return { ms, out: r.stdout, code: r.status };
};
const median = a => { const s = [...a].sort((x, y) => x - y); return s[Math.floor(s.length / 2)]; };
let totalNode = 0, totalRust = 0, slower = 0, differ = 0;
const ratios = [];
for (const c of cases) {
	const node = [], rust = [];
	let outN, outR;
	for (let i = 0; i < runs; i++) {
		const a = time("node", [eslint, ...c.args], c.stdin);
		const b = time(front, c.args, c.stdin);
		node.push(a.ms); rust.push(b.ms); outN = a; outR = b;
	}
	if (outN.out !== outR.out || outN.code !== outR.code) differ++;
	const mn = median(node), mr = median(rust);
	totalNode += mn; totalRust += mr; ratios.push(mn / mr);
	if (mr > mn) slower++;
}
console.log(JSON.stringify({ cases: cases.length, runs, node_total_ms: Math.round(totalNode), rust_total_ms: Math.round(totalRust), speedup_total: +(totalNode / totalRust).toFixed(2), median_ratio: +median(ratios).toFixed(2), min_ratio: +Math.min(...ratios).toFixed(2), slower_cases: slower, output_differs: differ }));
