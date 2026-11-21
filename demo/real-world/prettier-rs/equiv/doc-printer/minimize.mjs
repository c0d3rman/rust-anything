// Shrinks a mismatching doc + options to a minimal repro (greedy: subtree → child / leaf,
// list item removal, string shortening, smaller options) and writes it as JSON.
// Usage: node minimize.mjs <mismatch.jsonl> <line-number (1-based)> <out.json>
import fs from "node:fs";
import { rust } from "./lib.mjs";
import { jsPrint } from "./compare.mjs";

const [file, lineNo, outFile] = process.argv.slice(2);
const rec = JSON.parse(fs.readFileSync(file, "utf8").trim().split("\n")[Number(lineNo) - 1]);
const wire = o => (o.printWidth === Infinity || o.printWidth === null ? { ...o, printWidth: { $num: "Infinity" } } : o);
if (rec.opts.printWidth === null) rec.opts.printWidth = Infinity; // JSON.stringify(Infinity) is null
const run = (doc, opts) => {
  const s = JSON.stringify(doc);
  const js = jsPrint(s, [opts])[0];
  const r = JSON.parse(rust.printMany(s, JSON.stringify([wire(opts)])))[0];
  if (js[0] === "err" && r[0] === "err") return js[1] !== r[1] ? "errmsg" : null;
  return js.length !== r.length || js.some((x, i) => x !== r[i]) ? "output" : null;
};
const kind = run(rec.doc, rec.opts);
if (!kind) { console.error("does not reproduce"); process.exit(1); }
const size = d => JSON.stringify(d).length;
const clone = d => JSON.parse(JSON.stringify(d));
const fixCg = d => { if (d && d.cg) { if (d.expandedStates?.length) d.contents = clone(d.expandedStates[0]); else delete d.cg; } };

// all docs obtained from `d` by one shrinking step
function* shrinks(d) {
  if (typeof d === "string") { if (d.length) { yield ""; if ([...d].length > 1) yield [...d].slice(1).join(""); } return; }
  if (d === null || typeof d !== "object") return;
  if (Array.isArray(d)) {
    for (const x of d) yield x;
    for (let i = 0; i < d.length; i++) yield [...d.slice(0, i), ...d.slice(i + 1)];
    for (let i = 0; i < d.length; i++) for (const s of shrinks(d[i])) yield [...d.slice(0, i), s, ...d.slice(i + 1)];
    return;
  }
  yield "a"; yield "";
  for (const k of ["contents", "breakContents", "flatContents"]) if (k in d && !(k === "contents" && d.cg)) yield d[k];
  for (const k of ["parts", "expandedStates"]) if (Array.isArray(d[k])) {
    for (const x of d[k]) yield x;
    for (let i = 0; i < d[k].length; i++) { const e = clone(d); e[k].splice(i, 1); fixCg(e); yield e; }
    for (let i = 0; i < d[k].length; i++) for (const s of shrinks(d[k][i])) { const e = clone(d); e[k][i] = s; fixCg(e); yield e; }
  }
  for (const k of ["id", "groupId", "break", "negate"]) if (k in d) { const e = clone(d); delete e[k]; yield e; }
  if (d.cg) { const e = clone(d); delete e.cg; yield e; }
  for (const k of ["contents", "breakContents", "flatContents"]) if (k in d && !(k === "contents" && d.cg)) for (const s of shrinks(d[k])) { const e = clone(d); e[k] = s; yield e; }
}

let doc = rec.doc, opts = { ...rec.opts };
for (let progress = true; progress;) {
  progress = false;
  for (const c of shrinks(doc)) if (size(c) < size(doc) && run(c, opts) === kind) { doc = c; progress = true; break; }
  for (const [k, vals] of [["printWidth", [0, 1, 2, 3, 4, 5, 6, 8, 10, 80]], ["useTabs", [false]], ["tabWidth", [2]], ["endOfLine", ["lf"]]]) {
    for (const v of vals) { if (opts[k] === v) break; const o = { ...opts, [k]: v }; if (run(doc, o) === kind) { opts = o; progress = true; break; } }
  }
}
const s = JSON.stringify(doc);
const out = { kind, doc, options: opts, prettier: jsPrint(s, [opts])[0], rust: JSON.parse(rust.printMany(s, JSON.stringify([wire(opts)])))[0], note: "doc is in the port's JSON wire format; a group with \"cg\":1 is conditionalGroup(expandedStates), whose contents is the same object as expandedStates[0] on the Prettier side" };
fs.writeFileSync(outFile, JSON.stringify(out, null, 1) + "\n");
console.log(JSON.stringify(out));
