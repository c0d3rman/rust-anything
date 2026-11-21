// Bounded-exhaustive run over native `Doc` values (harness/src/native.rs): the representation
// the port's printers build. Rust enumerates each doc, prints it natively (propagate_breaks +
// print, no `from_value`) and writes it as Prettier's doc; here it is rebuilt with Prettier's
// object identity (`$def`/`$ref` → one object) and printed with `printDocToString`.
// Usage: node native-run.mjs --min 1 --max 5 [--worker k --workers W]
import fs from "node:fs";
import { here, rust, printDocToString } from "./lib.mjs";
import { OPTS_INDENT, OPTS_PLAIN } from "./compare.mjs";

const args = process.argv.slice(2);
const arg = (k, d) => { const i = args.indexOf(k); return i < 0 ? d : args[i + 1]; };
const min = Number(arg("--min", 1)), max = Number(arg("--max", 3));
const worker = Number(arg("--worker", 0)), workers = Number(arg("--workers", 1));
const roundTrip = args.includes("--round-trip");
const tag = `native-${min}-${max}-w${worker}of${workers}`;
const wire = o => o.map(x => x.printWidth === Infinity ? { ...x, printWidth: { $num: "Infinity" } } : x);
const OI = JSON.stringify(wire(OPTS_INDENT)), OP = JSON.stringify(wire(OPTS_PLAIN));
fs.mkdirSync(`${here}/out`, { recursive: true });
const mmFile = `${here}/out/mismatch-${tag}.jsonl`;
fs.writeFileSync(mmFile, "");

// Prettier's doc from the wire: `{$num}` → number, `{$def: k, doc}` / `{$ref: k}` → one object.
function materialize(text) {
  const defs = new Map();
  const rec = v => {
    if (Array.isArray(v)) return v.map(rec);
    if (!v || typeof v !== "object") return v;
    if ("$num" in v) return Number(v.$num);
    if ("$ref" in v) return defs.get(v.$ref);
    if ("$def" in v) { const d = rec(v.doc); defs.set(v.$def, d); return d; }
    const o = {};
    for (const [k, x] of Object.entries(v)) o[k] = rec(x);
    return o;
  };
  return rec(JSON.parse(text));
}

let docs = 0, prints = 0, skippedSym = 0, mismatches = 0, errBoth = 0, errMsgDiff = 0, rtBad = 0, saved = 0;
const perSize = {};
const t0 = Date.now();
const BATCH = 2000;
for (let n = min; n <= max; n++) {
  const total = BigInt(rust.nativeCount(n));
  const lo = total * BigInt(worker) / BigInt(workers), hi = total * BigInt(worker + 1) / BigInt(workers);
  const before = docs;
  for (let i = lo; i < hi; i += BigInt(BATCH)) {
    const count = Number(hi - i < BigInt(BATCH) ? hi - i : BigInt(BATCH));
    if (roundTrip) { const bad = rust.nativeRoundTrip(n, String(i), count); rtBad += bad.length; for (const b of bad.slice(0, 3)) if (saved++ < 2000) fs.appendFileSync(mmFile, JSON.stringify({ kind: "roundtrip", doc: JSON.parse(b) }) + "\n"); }
    const batch = JSON.parse(rust.nativeBatch(n, String(i), count, OI, OP));
    for (const e of batch) {
      if (!e) { skippedSym++; continue; }
      docs++;
      const opts = e.indent ? OPTS_INDENT : OPTS_PLAIN;
      const doc = materialize(e.doc);
      opts.forEach((o, k) => {
        prints++;
        let j;
        try { const r = printDocToString(doc, o); j = ["ok", r.formatted, r.cursorNodeStart ?? null, r.cursorNodeText ?? null]; }
        catch (err) { j = ["err", String(err?.message ?? err)]; }
        const r = e.rust[k];
        if (j[0] === "err" && r[0] === "err") {
          errBoth++;
          if (j[1] !== r[1]) { errMsgDiff++; if (saved++ < 2000) fs.appendFileSync(mmFile, JSON.stringify({ kind: "errmsg", doc: JSON.parse(e.doc), opts: o, js: j, rust: r }) + "\n"); }
          return;
        }
        if (j.length !== r.length || j.some((x, m) => x !== r[m])) {
          mismatches++;
          if (saved++ < 2000) fs.appendFileSync(mmFile, JSON.stringify({ kind: "output", doc: JSON.parse(e.doc), opts: o, js: j, rust: r }) + "\n");
        }
      });
    }
    if (docs % 200000 < BATCH) process.stderr.write(`[${tag}] size ${n}: ${docs} docs, ${prints} prints, ${mismatches} mismatches, ${errMsgDiff} errmsg, ${((Date.now() - t0) / 1000).toFixed(0)}s\n`);
  }
  perSize[n] = { enumerated: total.toString(), checkedByThisWorker: docs - before };
}
const summary = { alphabet: "native", min, max, worker, workers, perSize, docs, prints, skippedSym, mismatches, errBoth, errMsgDiff, ...(roundTrip ? { roundTripMismatches: rtBad } : {}), seconds: (Date.now() - t0) / 1000 };
fs.writeFileSync(`${here}/out/summary-${tag}.json`, JSON.stringify(summary, null, 1));
console.log(JSON.stringify(summary));
