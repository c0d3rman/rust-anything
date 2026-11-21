// Tallies driver output.  node summarize.mjs base out/base-*.jsonl | node summarize.mjs sweep out/sweep-*.jsonl
import fs from "node:fs";

const [mode, ...files] = process.argv.slice(2);
const rs = files.flatMap((f) => fs.readFileSync(f, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l)));
const table = {};
const bump = (lang, k) => ((table[lang] ??= {})[k] = (table[lang][k] ?? 0) + 1);
const notable = [];
for (const r of rs) {
  let cls;
  if (mode === "base") {
    if (r.skip) cls = "skip";
    else if (r.error) cls = "error";
    else if (r.port === "empty") cls = r.strEq ? "empty(str-equal)" : "empty(STR-DIFF)";
    else if (r.port !== "ok") cls = "port-declines";
    else if (r.prettierErr) cls = "prettier-error";
    else cls = r.docEq ? (r.strEq ? "doc-equal" : "DOC-EQ-STR-DIFF") : r.strEq ? "LATENT(doc-diff,str-equal)" : "STR-DIFF";
    if (r.port === "ok" && r.docEq && !r.strictEq) bump(r.lang, "  of which needed normalisation");
  } else {
    if (r.skip) cls = r.skip.startsWith("port") ? "port-declines" : r.skip === "large" ? "skip(>60k chars)" : "skip";
    else if (r.error) cls = "error";
    else {
      cls = r.docMismatchWidths.length ? "DOC-DIFF@some-width" : "doc-equal@all-widths";
      if (r.pDistinct > 1) bump(r.lang, "  prettier doc varies with width");
      if (r.strMismatchWidths?.length) bump(r.lang, "  STR-DIFF@some-width(0..200)");
      if (r.partial) bump(r.lang, "  partial");
    }
  }
  bump(r.lang, cls);
  if (/[A-Z]{3}/.test(cls) || r.error || r.strMismatchWidths?.length || r.partial) notable.push(r);
}
console.table(table);
const total = {};
for (const t of Object.values(table)) for (const [k, v] of Object.entries(t)) total[k] = (total[k] ?? 0) + v;
console.log("total", total);
for (const r of notable) console.log(JSON.stringify(r).slice(0, 900));
