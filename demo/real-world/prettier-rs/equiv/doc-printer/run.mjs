// Bounded-exhaustive run: every canonical doc of each size in [min, max] over an alphabet.
// Usage: node run.mjs --alphabet full --min 1 --max 4 [--worker k --workers W]
import fs from "node:fs";
import { here } from "./lib.mjs";
import { ALPHABETS, makeEnumerator, canonical, hasIndentation } from "./grammar.mjs";
import { Comparer } from "./compare.mjs";

const args = process.argv.slice(2);
const arg = (k, d) => { const i = args.indexOf(k); return i < 0 ? d : args[i + 1]; };
const alphaName = arg("--alphabet", "full");
const min = Number(arg("--min", 1)), max = Number(arg("--max", 3));
const worker = Number(arg("--worker", 0)), workers = Number(arg("--workers", 1));
const tag = `${alphaName}-${min}-${max}-w${worker}of${workers}`;
fs.mkdirSync(`${here}/out`, { recursive: true });
const mmFile = `${here}/out/mismatch-${tag}.jsonl`;
fs.writeFileSync(mmFile, "");
let saved = { output: 0, errmsg: 0 };
const cmp = new Comparer((kind, doc, opts, js, rs) => {
  if (saved[kind]++ < 2000) fs.appendFileSync(mmFile, JSON.stringify({ kind, doc: JSON.parse(doc), opts, js, rust: rs }) + "\n");
});
const en = makeEnumerator(ALPHABETS[alphaName]);
const t0 = Date.now();
const perSize = {};
let idx = 0, skippedSym = 0, lastLog = 0;
for (let n = min; n <= max; n++) {
  const before = cmp.docs;
  let enumerated = 0;
  for (const d of en.gen(n)) {
    enumerated++;
    if (!canonical(d)) { skippedSym++; continue; }
    if (idx++ % workers !== worker) continue;
    cmp.add(d, hasIndentation(d));
    if (cmp.docs - lastLog >= 200000) {
      lastLog = cmp.docs;
      process.stderr.write(`[${tag}] size ${n}: ${cmp.docs} docs, ${cmp.prints} prints, ${cmp.mismatches} mismatches, ${cmp.errMsgDiff} errmsg diffs, ${((Date.now() - t0) / 1000).toFixed(0)}s\n`);
    }
  }
  cmp.flushAll();
  perSize[n] = { enumerated, checkedByThisWorker: cmp.docs - before };
}
const summary = { alphabet: alphaName, min, max, worker, workers, perSize, docs: cmp.docs, prints: cmp.prints, skippedSym, mismatches: cmp.mismatches, errBoth: cmp.errBoth, errMsgDiff: cmp.errMsgDiff, seconds: (Date.now() - t0) / 1000 };
fs.writeFileSync(`${here}/out/summary-${tag}.json`, JSON.stringify(summary, null, 1));
console.log(JSON.stringify(summary));
