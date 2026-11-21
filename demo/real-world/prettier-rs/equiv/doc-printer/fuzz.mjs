// Random larger docs (deep nesting, long fills, many groups/ids) for a fixed time budget.
// Usage: node fuzz.mjs --seconds 600 --seed 1 [--max-size 300]
import fs from "node:fs";
import { here, rust } from "./lib.mjs";
import { jsPrint } from "./compare.mjs";

const args = process.argv.slice(2);
const arg = (k, d) => { const i = args.indexOf(k); return i < 0 ? d : Number(args[i + 1]); };
const seconds = arg("--seconds", 60), seed0 = arg("--seed", 1), maxSize = arg("--max-size", 300);
let seed = (seed0 * 2654435761) >>> 0 || 1;
const rnd = n => { seed ^= seed << 13; seed >>>= 0; seed ^= seed >>> 17; seed ^= seed << 5; seed >>>= 0; return seed % n; };
const pick = xs => xs[rnd(xs.length)];
const J = JSON.stringify;
const STR = ["", "a", "bb", "ccc", "word", "longerword", " ", "  ", "\t", "a ", " a", "字", "日本語", "😀", "👨‍👩‍👧", "é", "é", "x\ny", "ｆｕｌｌ", "🇺🇸", "1️⃣", ",", "(", ")", "{", "}", ";", "// c"];
const LEAF = ['{"type":"line"}', '{"type":"line","soft":true}', '[{"type":"line","hard":true},{"type":"break-parent"}]', '[{"type":"line","hard":true,"literal":true},{"type":"break-parent"}]', '{"type":"line","hard":true}', '{"type":"break-parent"}', '{"type":"trim"}', '{"type":"line-suffix-boundary"}', '{"type":"cursor"}'];
const IDS = ["A", "B", "C"];
const ALIGN = ['2', '1', '4', '-1', '0', '"> "', '"\\t"', '{"$num":"-Infinity"}', '{"type":"root"}'];

function gen(budget, depth) {
  if (budget <= 1 || rnd(10) < 2) {
    if (rnd(3) === 0) return pick(LEAF);
    return J(pick(STR));
  }
  const b = budget - 1;
  const split = n => { const xs = []; let left = b; while (left > 0 && xs.length < n) { const s = 1 + rnd(Math.max(1, Math.min(left, Math.ceil(b / 2) + 1))); xs.push(gen(Math.min(s, left), depth + 1)); left -= s; } return xs; };
  switch (rnd(14)) {
    case 0: case 1: return `[${split(2 + rnd(6)).join(",")}]`;
    case 2: return `{"type":"indent","contents":${gen(b, depth + 1)}}`;
    case 3: return `{"type":"align","n":${pick(ALIGN)},"contents":${gen(b, depth + 1)}}`;
    case 4: case 5: {
      const id = rnd(3) === 0 ? `"id":"${pick(IDS)}",` : "";
      return `{"type":"group",${id}"contents":${gen(b, depth + 1)},"break":${rnd(6) === 0}}`;
    }
    case 6: { // long fill: alternating content / separator
      const parts = []; const n = 1 + rnd(Math.min(40, b));
      for (let i = 0; i < n; i++) parts.push(i % 2 ? pick(['{"type":"line"}', '{"type":"line","soft":true}', '[{"type":"line","hard":true},{"type":"break-parent"}]', gen(2, depth + 1)]) : (rnd(4) ? J(pick(STR)) : gen(Math.max(1, b / n | 0), depth + 1)));
      return `{"type":"fill","parts":[${parts.join(",")}]}`;
    }
    case 7: { const g = rnd(2) ? `"groupId":"${pick(IDS)}",` : ""; const [x, y] = [gen(b >> 1, depth + 1), gen(b >> 1, depth + 1)]; return `{"type":"if-break",${g}"breakContents":${x},"flatContents":${y}}`; }
    case 8: { const g = rnd(4) ? `"groupId":"${pick(IDS)}",` : ""; return `{"type":"indent-if-break",${g}"contents":${gen(b, depth + 1)},"negate":${rnd(3) === 0}}`; }
    case 9: return `{"type":"line-suffix","contents":${gen(Math.min(b, 4), depth + 1)}}`;
    case 10: return `{"type":"label","label":"x","contents":${gen(b, depth + 1)}}`;
    case 11: { const states = split(2 + rnd(3)); const id = rnd(4) === 0 ? `"id":"${pick(IDS)}",` : ""; return `{"type":"group","cg":1,${id}"contents":${states[0]},"break":${rnd(8) === 0},"expandedStates":[${states.join(",")}]}`; }
    case 12: { // deep chain of groups/indents
      let d = gen(Math.max(1, b - 20), depth + 20);
      for (let i = 0; i < Math.min(20, b); i++) d = rnd(2) ? `{"type":"group","contents":[${J(pick(STR))},{"type":"line","soft":true},${d}],"break":false}` : `{"type":"indent","contents":[{"type":"line"},${d}]}`;
      return d;
    }
    default: return `[${split(2 + rnd(3)).join(",")}]`;
  }
}

const mmFile = `${here}/out/fuzz-mismatch-seed${seed0}.jsonl`;
fs.writeFileSync(mmFile, "");
const t0 = Date.now();
let docs = 0, prints = 0, mismatches = 0, errBoth = 0, errMsgDiff = 0, saved = 0, totalSize = 0;
while (Date.now() - t0 < seconds * 1000) {
  const opts = Array.from({ length: 6 }, () => ({ printWidth: pick([0, 1, 2, 3, 4, 5, 6, 8, 10, 12, 16, 20, 30, 40, 80]), tabWidth: pick([1, 2, 4, 8]), useTabs: rnd(2) === 0, endOfLine: pick(["lf", "lf", "lf", "crlf", "cr"]) }));
  const batch = Array.from({ length: 200 }, () => gen(1 + rnd(maxSize), 0));
  const rs = JSON.parse(rust.printBatch(`[${batch.join(",")}]`, JSON.stringify(opts)));
  batch.forEach((d, i) => {
    docs++; totalSize += d.length;
    const js = jsPrint(d, opts);
    js.forEach((j, k) => {
      prints++;
      const r = rs[i][k];
      let kind = null;
      if (j[0] === "err" && r[0] === "err") { errBoth++; if (j[1] !== r[1]) { errMsgDiff++; kind = "errmsg"; } }
      else if (j.length !== r.length || j.some((x, m) => x !== r[m])) { mismatches++; kind = "output"; }
      if (kind && saved++ < 500) fs.appendFileSync(mmFile, JSON.stringify({ kind, doc: JSON.parse(d), opts: opts[k], js: j, rust: r }) + "\n");
    });
  });
}
const summary = { seed: seed0, seconds, maxSize, docs, prints, avgJsonBytes: Math.round(totalSize / docs), mismatches, errBoth, errMsgDiff };
fs.writeFileSync(`${here}/out/fuzz-summary-seed${seed0}.json`, JSON.stringify(summary, null, 1));
console.log(JSON.stringify(summary));
