// Attributes MISMATCH findings to known causes by re-running controlled variants, so that
// minimization can be spent on the ones no known cause explains. For each finding it builds:
//   es5     --trailing-comma es5 -> all          (TS type-parameter trailing comma)
//   nocomm  every comment blanked to spaces      (comment attachment / printing)
//   asts    a .js/.jsx file renamed to .ts/.tsx  (Babel-vs-ESTree tree shape; only if it parses as TS)
// A finding is explained by the first variant that makes both sides agree.
// Usage: node triage.mjs FINDINGS.jsonl OUT_DIR   (needs $B = equiv-fuzz binary)
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { requireP } from "./gen/common.mjs";

const babel = requireP("@babel/parser");
const [file, outDir] = process.argv.slice(2);
const B = process.env.B;
if (!B) {
  console.error("Set B to the equiv-fuzz driver binary (target/release/equiv-fuzz).");
  process.exit(2);
}
fs.mkdirSync(outDir, { recursive: true });
const findings = fs.readFileSync(file, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l))
  .filter((x) => x.category === "MISMATCH" || x.idem === "nonidem_diverges");

function blankComments(text, ts, jsx) {
  let ast;
  try {
    ast = babel.parse(text, { sourceType: "module", errorRecovery: true, allowReturnOutsideFunction: true,
      allowAwaitOutsideFunction: true, plugins: [...(ts ? ["typescript"] : []), ...(jsx ? ["jsx"] : []), "decorators-legacy"] });
  } catch { return null; }
  if (!ast.comments.length) return null;
  let out = text;
  for (const c of ast.comments) out = out.slice(0, c.start) + out.slice(c.start, c.end).replace(/[^\n]/g, " ") + out.slice(c.end);
  return out;
}

const VARIANTS = ["es5", "nocomm", "asts"];
const cases = [];
findings.forEach((x, i) => {
  const fp = x.args[x.args.indexOf("--stdin-filepath") + 1];
  const ts = /\.(ts|tsx|mts|cts)$/i.test(fp), jsx = /\.(jsx|tsx|js)$/i.test(fp);
  const stdin = x.idem === "nonidem_diverges" ? x.rust.out : x.stdin;
  const es5 = x.args.indexOf("es5");
  if (es5 >= 0) { const a = x.args.slice(); a[es5] = "all"; cases.push({ name: `${i}#es5`, args: a, stdin }); }
  const nc = /\.(js|jsx|mjs|cjs|ts|tsx|mts|cts)$/i.test(fp) ? blankComments(stdin, ts, jsx) : null;
  if (nc != null) cases.push({ name: `${i}#nocomm`, args: x.args, stdin: nc });
  if (/\.(js|jsx|mjs|cjs)$/i.test(fp)) {
    const a = x.args.slice();
    a[a.indexOf("--stdin-filepath") + 1] = fp.replace(/\.(js|mjs|cjs)$/i, ".ts").replace(/\.jsx$/i, ".tsx");
    cases.push({ name: `${i}#asts`, args: a, stdin });
  }
});
const inFile = path.join(outDir, "variants.jsonl"), logFile = path.join(outDir, "variants-log.jsonl");
fs.writeFileSync(inFile, cases.map((c) => JSON.stringify(c)).join("\n") + "\n");
execFileSync(B, ["run", "--gen", "triage", "--in", inFile, "--out", path.join(outDir, "run"), "--workers", process.env.WORKERS ?? "2", "--log-all", logFile], { stdio: ["ignore", "ignore", "inherit"] });
const res = new Map(fs.readFileSync(logFile, "utf8").split("\n").filter(Boolean).map((l) => { const r = JSON.parse(l); return [r.name, r]; }));
const counts = {};
const residual = [];
findings.forEach((x, i) => {
  const cause = VARIANTS.find((v) => res.get(`${i}#${v}`)?.category === "equal" && res.get(`${i}#${v}`)?.idem !== "nonidem_diverges") ?? "unexplained";
  const k = `${x.lang}/${cause}`;
  counts[k] = (counts[k] ?? 0) + 1;
  fs.appendFileSync(path.join(outDir, `${cause}.jsonl`), JSON.stringify(x) + "\n");
  if (cause === "unexplained") residual.push(x);
});
console.log(counts);
