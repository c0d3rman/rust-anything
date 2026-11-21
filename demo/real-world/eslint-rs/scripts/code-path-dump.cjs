// ESLINT_CLONE=<clone> node code-path-dump.cjs <names.txt> <app-cases dir> <out dir>: every enter/leave step and code path
// event (with each code path's segment graph) of ESLint's CodePathAnalyzer, as linter-rs/examples/cpa_events.rs prints them.
"use strict";
const clone = process.env.ESLINT_CLONE;
const fs = require("fs");
const path = require("path");
const { Linter } = require(`${clone}/lib/api`);
const [listFile, casesDir, outDir] = process.argv.slice(2);
fs.mkdirSync(outDir, { recursive: true });

function makeRule(out) {
	return {
		create(context) {
			const seen = new Map();
			const nd = n => `${n.type} ${n.range[0]}-${n.range[1]}`;
			const see = s => {
				const cp = s.id.split("_")[0];
				if (!seen.has(cp)) seen.set(cp, []);
				seen.get(cp).push(s);
			};
			const ids = a => a.map(s => s.id).join(",");
			const num = s => Number(s.id.split("_").at(-1));
			const segEv = name => (s, n) => { see(s); out.push(`${name} ${s.id} ${nd(n)}`); };
			return {
				"*"(n) { out.push(`enter ${nd(n)}`); },
				"*:exit"(n) { out.push(`exit ${nd(n)}`); },
				onCodePathStart(cp, n) { out.push(`onCodePathStart ${cp.id} ${cp.origin} ${nd(n)}`); },
				onCodePathEnd(cp, n) {
					out.push(`onCodePathEnd ${cp.id} ${nd(n)} initial=${cp.initialSegment.id} final=[${ids(cp.finalSegments)}] returned=[${ids(cp.returnedSegments)}] thrown=[${ids(cp.thrownSegments)}]`);
					const set = [cp.initialSegment, ...cp.finalSegments, ...cp.returnedSegments, ...cp.thrownSegments, ...(seen.get(cp.id) || [])];
					const uniq = [];
					for (const s of set) if (!uniq.includes(s)) uniq.push(s);
					for (let i = 0; i < uniq.length; i++) {
						const s = uniq[i];
						for (const x of [...s.allNextSegments, ...s.allPrevSegments, ...s.nextSegments, ...s.prevSegments]) if (!uniq.includes(x)) uniq.push(x);
					}
					uniq.sort((a, b) => num(a) - num(b));
					for (const x of uniq) {
						out.push(`  seg ${x.id} reachable=${x.reachable} prev=[${ids(x.prevSegments)}] next=[${ids(x.nextSegments)}] allPrev=[${ids(x.allPrevSegments)}] allNext=[${ids(x.allNextSegments)}] looped=[${ids(x.internal.loopedPrevSegments)}]`);
					}
				},
				onCodePathSegmentStart: segEv("onCodePathSegmentStart"),
				onCodePathSegmentEnd: segEv("onCodePathSegmentEnd"),
				onUnreachableCodePathSegmentStart: segEv("onUnreachableCodePathSegmentStart"),
				onUnreachableCodePathSegmentEnd: segEv("onUnreachableCodePathSegmentEnd"),
				onCodePathSegmentLoop(a, b, n) { out.push(`onCodePathSegmentLoop ${a.id} ${b.id} ${nd(n)}`); },
			};
		},
	};
}

const names = fs.readFileSync(listFile, "utf8").split("\n").filter(Boolean);
let ok = 0, skipped = 0;
for (const name of names) {
	const toml = fs.readFileSync(path.join(casesDir, name + ".toml"), "utf8");
	const code = fs.readFileSync(path.join(casesDir, name + ".stdin"), "utf8");
	const fm = toml.match(/"--stdin-filename", "([^"]+)"/);
	const filename = fm ? fm[1] : "input.js";
	let jsx = false, globalReturn = false;
	const pm = toml.match(/"--parser-options", '([^']*)'/);
	if (pm) {
		jsx = /"jsx":true/.test(pm[1]); globalReturn = /"globalReturn":true/.test(pm[1]);
		if (/impliedStrict|ecmaVersion|sourceType/.test(pm[1])) { skipped++; continue; }
	}
	if (/"--parser"|"--env"/.test(toml)) { skipped++; continue; }
	const sourceType = filename.endsWith(".cjs") ? "commonjs" : "module";
	const out = [];
	const linter = new Linter({ configType: "flat" });
	const msgs = linter.verify(code, [{
		plugins: { d: { rules: { dump: makeRule(out) } } },
		rules: { "d/dump": "error" },
		languageOptions: { sourceType, parserOptions: { ecmaFeatures: { jsx, globalReturn } } },
	}], { filename });
	if (msgs.some(m => m.fatal)) { skipped++; continue; }
	fs.writeFileSync(path.join(outDir, name + ".txt"), `${filename} ${jsx} ${globalReturn}\n` + out.join("\n") + "\n");
	ok++;
}
console.log(`dumped ${ok}, skipped ${skipped}`);
